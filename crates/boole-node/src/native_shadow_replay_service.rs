//! Feature-gated, closed-local native-shadow adjudication service.
//!
//! This owns a dedicated loopback router. It is never merged into the normal
//! node router and has no block, reward, P2P, consensus or mining activation
//! dependency.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{to_bytes, Body, Bytes};
use axum::extract::State;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::native_shadow::{
    ChallengeState, NativeShadowAdmissionError, NativeShadowAdmissionView, NativeShadowEvidence,
    NativeShadowEvidenceVerdict, NativeShadowExecutionGate, NativeShadowExhaustionLedger,
    NativeShadowFourTuple, NativeShadowGrantAttemptErrorV1, NativeShadowGrantAttemptFieldsV1,
    NativeShadowGrantAttemptKindV1, NativeShadowJournalAuthority, NativeShadowStateStore,
    NativeShadowTerminalReceiptBinding, ResolveOutcome, VerifiedNativeShadowReplayBootstrap,
};
use crate::native_shadow_submission::{
    NativeShadowIntakeError, NativeShadowSubmission, NativeShadowSubmissionSource,
};

const SUBMISSION_ROUTE: &str = "/native-shadow/submissions";
const HTTP_BODY_LIMIT_BYTES: usize = 131_072;
const HTTP_BODY_TIMEOUT: Duration = Duration::from_secs(5);
const HTTP_TOTAL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(115);
const FIXED_HTTP_LISTENER_PORT: u16 = 8082;
#[cfg(any(target_os = "linux", test))]
const LAUNCHER_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
#[cfg(any(target_os = "linux", test))]
const LAUNCHER_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(any(target_os = "linux", test))]
const LAUNCHER_EXECUTION_TIMEOUT: Duration = Duration::from_secs(115);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyReadFailure {
    Timeout,
    TooLargeOrUnreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnerResponseFailure {
    Deadline,
    OwnerDropped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttemptReservationDisposition {
    ChallengeExhausted,
    Poison,
}

fn classify_attempt_reservation_error(
    error: &NativeShadowGrantAttemptErrorV1,
) -> AttemptReservationDisposition {
    match error {
        NativeShadowGrantAttemptErrorV1::CaseAlreadyReserved
        | NativeShadowGrantAttemptErrorV1::TotalBudgetExceeded
        | NativeShadowGrantAttemptErrorV1::CheckerBudgetExceeded => {
            AttemptReservationDisposition::ChallengeExhausted
        }
        NativeShadowGrantAttemptErrorV1::InvalidDigest(_)
        | NativeShadowGrantAttemptErrorV1::ScopeDrift
        | NativeShadowGrantAttemptErrorV1::OperationAlreadyReserved
        | NativeShadowGrantAttemptErrorV1::Durability(_) => AttemptReservationDisposition::Poison,
    }
}

#[derive(Debug, Clone)]
struct ReplayCaseIdentity {
    four_tuple: NativeShadowFourTuple,
    registry_version: String,
    registry_digest: String,
    execution_policy_digest: crate::native_shadow::NativeShadowExecutionPolicyDigest,
    operation_id_hex: String,
    candidate_digest: String,
    submission_digest: String,
    anchor_digest: String,
    intake_version: String,
    checker_digest: String,
    checker_policy_digest: String,
    toolchain_digest: String,
}

#[derive(Debug, Clone)]
struct ReplayRedeliveryIdentity {
    four_tuple: NativeShadowFourTuple,
    registry_version: String,
    registry_digest: String,
    execution_policy_digest: crate::native_shadow::NativeShadowExecutionPolicyDigest,
    candidate_digest: String,
    submission_digest: String,
}

impl From<&ReplayCaseIdentity> for ReplayRedeliveryIdentity {
    fn from(identity: &ReplayCaseIdentity) -> Self {
        Self {
            four_tuple: identity.four_tuple.clone(),
            registry_version: identity.registry_version.clone(),
            registry_digest: identity.registry_digest.clone(),
            execution_policy_digest: identity.execution_policy_digest.clone(),
            candidate_digest: identity.candidate_digest.clone(),
            submission_digest: identity.submission_digest.clone(),
        }
    }
}

#[derive(Debug)]
struct PreparedCheckerCase<P, R> {
    prepared: P,
    request: R,
    identity: ReplayCaseIdentity,
}

#[derive(Debug)]
struct PreparedPreIntakeCase<P> {
    prepared: P,
    four_tuple: NativeShadowFourTuple,
    registry_digest: String,
    execution_policy_digest: crate::native_shadow::NativeShadowExecutionPolicyDigest,
    operation_id_hex: String,
    candidate_digest: String,
    submission_digest: String,
}

#[derive(Debug)]
struct AuthorizedCheckerCase<R> {
    request: R,
    identity: ReplayCaseIdentity,
    bootstrap: VerifiedNativeShadowReplayBootstrap,
}

trait ReplayAuthority: Send + Sync + 'static {
    type CheckerPrepared: Send + 'static;
    type PreIntakePrepared: Send + 'static;
    type Request: Send + Sync + 'static;

    fn redelivery_identity(
        &self,
        submission: &NativeShadowSubmission,
    ) -> Result<ReplayRedeliveryIdentity, &'static str>;

    fn prepare_checker(
        &self,
        submission: &NativeShadowSubmission,
        source: &NativeShadowSubmissionSource,
    ) -> Result<PreparedCheckerCase<Self::CheckerPrepared, Self::Request>, &'static str>;

    fn validate_checker_before_reserve(
        &self,
        prepared: &PreparedCheckerCase<Self::CheckerPrepared, Self::Request>,
    ) -> Result<(), &'static str>;

    fn authorize_checker(
        &self,
        prepared: PreparedCheckerCase<Self::CheckerPrepared, Self::Request>,
    ) -> Result<AuthorizedCheckerCase<Self::Request>, &'static str>;

    fn prepare_pre_intake(
        &self,
        submission: &NativeShadowSubmission,
    ) -> Result<PreparedPreIntakeCase<Self::PreIntakePrepared>, &'static str>;

    fn validate_pre_intake_before_reserve(
        &self,
        prepared: &PreparedPreIntakeCase<Self::PreIntakePrepared>,
    ) -> Result<(), &'static str>;

    fn authorize_pre_intake(&self, prepared: Self::PreIntakePrepared) -> Result<(), &'static str>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValidatedLauncherVerdict {
    Accepted,
    DeterministicReject,
}

#[derive(Debug)]
struct ValidatedLauncherTerminal {
    verdict: ValidatedLauncherVerdict,
    reason_code: &'static str,
}

#[derive(Debug)]
enum ValidatedLauncherOutcome {
    Terminal(ValidatedLauncherTerminal),
    RetryableClean {
        reason: crate::native_shadow::NativeShadowRetryableReasonV3,
    },
    Ambiguous {
        reason_code: &'static str,
    },
}

trait LauncherTransport<R>: Send + Sync + 'static {
    fn execute(&self, request: &R) -> ValidatedLauncherOutcome;
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LauncherPeerIdentity {
    pid: u32,
    uid: u32,
    gid: u32,
}

#[cfg(any(target_os = "linux", test))]
fn validate_qualified_launcher_identity(
    qualified_pid: u32,
    qualified_instance_id_hex: &str,
    peer: LauncherPeerIdentity,
    ready_pid: u32,
    ready_instance_id_hex: &str,
) -> Result<(), &'static str> {
    if peer.pid == 0 || peer.uid != 0 || peer.gid != 0 {
        return Err("execution_peer_not_root");
    }
    if peer.pid != qualified_pid || ready_pid != qualified_pid {
        return Err("execution_launcher_pid_drift");
    }
    if ready_instance_id_hex != qualified_instance_id_hex {
        return Err("execution_launcher_instance_drift");
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, Copy)]
struct LauncherAdjudicationFacts {
    cleanup_complete: bool,
    timed_out: bool,
    signaled: bool,
    memory_events_max_delta: u64,
    pids_events_max_delta: u64,
    output_limit_exceeded: bool,
    checker_verdict: Option<boole_native_shadow_protocol::CheckerVerdict>,
    checker_reason: Option<boole_native_shadow_protocol::CheckerReason>,
}

#[cfg(any(target_os = "linux", test))]
fn classify_launcher_facts(facts: LauncherAdjudicationFacts) -> ValidatedLauncherOutcome {
    use boole_native_shadow_protocol::{CheckerReason, CheckerVerdict};

    if !facts.cleanup_complete {
        return ValidatedLauncherOutcome::Ambiguous {
            reason_code: "launcher_cleanup_incomplete",
        };
    }
    if facts.timed_out {
        return ValidatedLauncherOutcome::RetryableClean {
            reason: crate::native_shadow::NativeShadowRetryableReasonV3::ContainmentWallClockKill,
        };
    }
    if facts.signaled {
        return ValidatedLauncherOutcome::RetryableClean {
            reason: crate::native_shadow::NativeShadowRetryableReasonV3::ContainmentKilled,
        };
    }
    match (facts.checker_verdict, facts.checker_reason) {
        (Some(CheckerVerdict::Accepted), Some(CheckerReason::Accepted)) => {
            ValidatedLauncherOutcome::Terminal(ValidatedLauncherTerminal {
                verdict: ValidatedLauncherVerdict::Accepted,
                reason_code: "accepted",
            })
        }
        (Some(CheckerVerdict::DeterministicReject), Some(_)) => {
            ValidatedLauncherOutcome::Terminal(ValidatedLauncherTerminal {
                verdict: ValidatedLauncherVerdict::DeterministicReject,
                reason_code: "checker_rejected",
            })
        }
        (
            Some(CheckerVerdict::RetryableUnavailable),
            Some(CheckerReason::ResourceProcessLimit),
        ) => ValidatedLauncherOutcome::Terminal(ValidatedLauncherTerminal {
            verdict: ValidatedLauncherVerdict::DeterministicReject,
            reason_code: if facts.pids_events_max_delta > 0 {
                "submission_resource_ceiling_breach"
            } else {
                "checker_reported_reason_unconfirmed"
            },
        }),
        (
            Some(CheckerVerdict::RetryableUnavailable),
            Some(CheckerReason::ResourceMemoryLimit),
        ) => ValidatedLauncherOutcome::Terminal(ValidatedLauncherTerminal {
            verdict: ValidatedLauncherVerdict::DeterministicReject,
            reason_code: if facts.memory_events_max_delta > 0 {
                "submission_resource_ceiling_breach"
            } else {
                "checker_reported_reason_unconfirmed"
            },
        }),
        (
            Some(CheckerVerdict::RetryableUnavailable),
            Some(CheckerReason::ResourceWallLimit),
        ) => ValidatedLauncherOutcome::RetryableClean {
            reason: crate::native_shadow::NativeShadowRetryableReasonV3::ContainmentWallClockKill,
        },
        (
            Some(CheckerVerdict::RetryableUnavailable),
            Some(CheckerReason::ResourceProcessTerminated),
        ) => ValidatedLauncherOutcome::RetryableClean {
            reason: crate::native_shadow::NativeShadowRetryableReasonV3::ContainmentKilled,
        },
        (
            Some(CheckerVerdict::RetryableUnavailable),
            Some(CheckerReason::CheckerInternalError),
        ) => ValidatedLauncherOutcome::RetryableClean {
            reason: crate::native_shadow::NativeShadowRetryableReasonV3::CheckerInternalError,
        },
        _ if facts.output_limit_exceeded => ValidatedLauncherOutcome::RetryableClean {
            reason:
                crate::native_shadow::NativeShadowRetryableReasonV3::ContainmentEnvironmentUnavailable,
        },
        _ => ValidatedLauncherOutcome::RetryableClean {
            reason:
                crate::native_shadow::NativeShadowRetryableReasonV3::ContainmentEnvironmentUnavailable,
        },
    }
}

#[cfg(target_os = "linux")]
fn classify_validated_launcher_session(
    session: &boole_native_shadow_protocol::ValidatedClosedLocalReplayExecutionSession,
) -> ValidatedLauncherOutcome {
    let view = session.report().adjudication_view();
    classify_launcher_facts(LauncherAdjudicationFacts {
        cleanup_complete: session.cleanup_complete(),
        timed_out: view.timed_out,
        signaled: view.signaled,
        memory_events_max_delta: view.memory_events_max_delta,
        pids_events_max_delta: view.pids_events_max_delta,
        output_limit_exceeded: view.output_limit_exceeded,
        checker_verdict: view.checker_verdict,
        checker_reason: view.checker_reason,
    })
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct InstalledReplayAuthority {
    installed: Arc<
        boole_native_shadow_protocol::installed_authority::VerifiedInstalledClosedLocalReplayExecutionAuthorities,
    >,
}

#[cfg(target_os = "linux")]
impl InstalledReplayAuthority {
    fn new(
        installed: boole_native_shadow_protocol::installed_authority::VerifiedInstalledClosedLocalReplayExecutionAuthorities,
    ) -> Self {
        Self {
            installed: Arc::new(installed),
        }
    }
}

#[cfg(target_os = "linux")]
impl ReplayAuthority for InstalledReplayAuthority {
    type CheckerPrepared = boole_native_shadow_protocol::VerifiedClosedLocalReplayPreparedCase;
    type PreIntakePrepared =
        boole_native_shadow_protocol::VerifiedClosedLocalReplayPreparedPreIntakeCase;
    type Request = boole_native_shadow_protocol::ExecutionRequest;

    fn redelivery_identity(
        &self,
        submission: &NativeShadowSubmission,
    ) -> Result<ReplayRedeliveryIdentity, &'static str> {
        Ok(ReplayRedeliveryIdentity {
            four_tuple: NativeShadowFourTuple {
                family_version: submission.family_version().to_string(),
                template_id: submission.template_id().to_string(),
                challenge_sha256: submission.challenge_sha256().to_string(),
                epoch: submission.epoch(),
            },
            registry_version: self.installed.grant().registry_version().to_string(),
            registry_digest: self.installed.grant().registry_digest_hex().to_string(),
            execution_policy_digest:
                crate::native_shadow::NativeShadowExecutionPolicyDigest::try_from(
                    self.installed
                        .execution_authority()
                        .base_execution_policy_sha256(),
                )
                .map_err(|_| "execution_policy_digest_invalid")?,
            candidate_digest: submission.candidate_digest_hex(),
            submission_digest: submission.submission_digest_hex(),
        })
    }

    fn prepare_checker(
        &self,
        submission: &NativeShadowSubmission,
        source: &NativeShadowSubmissionSource,
    ) -> Result<PreparedCheckerCase<Self::CheckerPrepared, Self::Request>, &'static str> {
        use boole_native_shadow_protocol::ClosedLocalReplaySubmissionFields;

        let prepared = self
            .installed
            .grant()
            .prepare_execution_case(ClosedLocalReplaySubmissionFields {
                family_version: submission.family_version(),
                template_id: submission.template_id(),
                challenge_sha256: submission.challenge_sha256(),
                epoch: submission.epoch(),
                candidate_digest_hex: &submission.candidate_digest_hex(),
                submission_source_digest_hex: source.digest_hex(),
            })
            .map_err(|_| "grant_case_mismatch")?;
        let nonce_hex = linux_fresh_nonce_hex().map_err(|_| "nonce_unavailable")?;
        let request = prepared
            .build_execution_request(
                &nonce_hex,
                submission.raw_answer_bytes(),
                source.source().as_bytes(),
            )
            .map_err(|_| "request_binding_mismatch")?;
        let identity = ReplayCaseIdentity {
            four_tuple: NativeShadowFourTuple {
                family_version: request.family_version().to_string(),
                template_id: request.template_id().to_string(),
                challenge_sha256: request.challenge_sha256().to_string(),
                epoch: request.epoch(),
            },
            registry_version: request.registry_version().to_string(),
            registry_digest: request.registry_digest_hex().to_string(),
            execution_policy_digest:
                crate::native_shadow::NativeShadowExecutionPolicyDigest::try_from(
                    request.execution_policy_digest_hex(),
                )
                .map_err(|_| "execution_policy_digest_invalid")?,
            operation_id_hex: request.operation_id_hex().to_string(),
            candidate_digest: request.candidate_digest_hex().to_string(),
            submission_digest: request.submission_digest_hex().to_string(),
            anchor_digest: request.anchor_digest_hex().to_string(),
            intake_version: request.intake_version().to_string(),
            checker_digest: request.checker_artifact_hash_hex().to_string(),
            checker_policy_digest: request.checker_policy_digest_hex().to_string(),
            toolchain_digest: request.toolchain_identity_digest_hex().to_string(),
        };
        Ok(PreparedCheckerCase {
            prepared,
            request,
            identity,
        })
    }

    fn authorize_checker(
        &self,
        prepared: PreparedCheckerCase<Self::CheckerPrepared, Self::Request>,
    ) -> Result<AuthorizedCheckerCase<Self::Request>, &'static str> {
        let authorization = self
            .installed
            .grant()
            .authorize_prepared_execution_request(prepared.prepared, &prepared.request)
            .map_err(|_| "grant_authorization_failed")?;
        let bootstrap = VerifiedNativeShadowReplayBootstrap::from_authorization(&authorization)
            .map_err(|_| "bootstrap_binding_invalid")?;
        Ok(AuthorizedCheckerCase {
            request: prepared.request,
            identity: prepared.identity,
            bootstrap,
        })
    }

    fn validate_checker_before_reserve(
        &self,
        prepared: &PreparedCheckerCase<Self::CheckerPrepared, Self::Request>,
    ) -> Result<(), &'static str> {
        let identity = &prepared.identity;
        self.installed
            .grant()
            .matches_durable_attempt(
                boole_native_shadow_protocol::DurableClosedLocalReplayAttemptFields {
                    family_version: &identity.four_tuple.family_version,
                    template_id: &identity.four_tuple.template_id,
                    challenge_sha256: &identity.four_tuple.challenge_sha256,
                    epoch: identity.four_tuple.epoch,
                    operation_id_hex: &identity.operation_id_hex,
                    candidate_digest_hex: &identity.candidate_digest,
                    submission_digest_hex: &identity.submission_digest,
                    pre_intake_only: false,
                },
            )
            .then_some(())
            .ok_or("grant_case_mismatch_before_reserve")
    }

    fn prepare_pre_intake(
        &self,
        submission: &NativeShadowSubmission,
    ) -> Result<PreparedPreIntakeCase<Self::PreIntakePrepared>, &'static str> {
        use boole_native_shadow_protocol::{
            ClosedLocalReplayPreIntakeFields, ClosedLocalReplayPreIntakeReason,
        };

        let candidate_digest = submission.candidate_digest_hex();
        let prepared = self
            .installed
            .grant()
            .prepare_pre_intake_case(ClosedLocalReplayPreIntakeFields {
                family_version: submission.family_version(),
                template_id: submission.template_id(),
                challenge_sha256: submission.challenge_sha256(),
                epoch: submission.epoch(),
                candidate_digest_hex: &candidate_digest,
                reason: ClosedLocalReplayPreIntakeReason::EmptyResponse,
            })
            .map_err(|_| "grant_pre_intake_mismatch")?;
        let operation_id_hex = prepared.operation_id_hex().to_string();
        Ok(PreparedPreIntakeCase {
            prepared,
            four_tuple: NativeShadowFourTuple {
                family_version: submission.family_version().to_string(),
                template_id: submission.template_id().to_string(),
                challenge_sha256: submission.challenge_sha256().to_string(),
                epoch: submission.epoch(),
            },
            registry_digest: self.installed.grant().registry_digest_hex().to_string(),
            execution_policy_digest:
                crate::native_shadow::NativeShadowExecutionPolicyDigest::try_from(
                    self.installed
                        .execution_authority()
                        .base_execution_policy_sha256(),
                )
                .map_err(|_| "execution_policy_digest_invalid")?,
            operation_id_hex,
            candidate_digest,
            submission_digest: submission.submission_digest_hex(),
        })
    }

    fn authorize_pre_intake(&self, prepared: Self::PreIntakePrepared) -> Result<(), &'static str> {
        self.installed
            .grant()
            .authorize_prepared_pre_intake_case(prepared)
            .map(|_| ())
            .map_err(|_| "grant_pre_intake_authorization_failed")
    }

    fn validate_pre_intake_before_reserve(
        &self,
        prepared: &PreparedPreIntakeCase<Self::PreIntakePrepared>,
    ) -> Result<(), &'static str> {
        self.installed
            .grant()
            .matches_durable_attempt(
                boole_native_shadow_protocol::DurableClosedLocalReplayAttemptFields {
                    family_version: &prepared.four_tuple.family_version,
                    template_id: &prepared.four_tuple.template_id,
                    challenge_sha256: &prepared.four_tuple.challenge_sha256,
                    epoch: prepared.four_tuple.epoch,
                    operation_id_hex: &prepared.operation_id_hex,
                    candidate_digest_hex: &prepared.candidate_digest,
                    submission_digest_hex: &prepared.submission_digest,
                    pre_intake_only: true,
                },
            )
            .then_some(())
            .ok_or("grant_pre_intake_mismatch_before_reserve")
    }
}

#[cfg(target_os = "linux")]
mod installed_launcher_transport {
    use std::io::{self, Read, Write};
    use std::mem::{self, MaybeUninit};
    use std::net::Shutdown;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use boole_native_shadow_protocol::{
        encode_execution_request_frame, read_closed_local_replay_execution_ready,
        read_execution_report, validate_closed_local_replay_execution_session,
        write_execution_hello, write_execution_request, ExecutionHello, ExecutionRequest,
    };

    use super::{
        classify_validated_launcher_session, validate_qualified_launcher_identity,
        LauncherPeerIdentity, LauncherTransport, ValidatedLauncherOutcome,
        LAUNCHER_CONNECT_TIMEOUT, LAUNCHER_EXECUTION_TIMEOUT, LAUNCHER_HANDSHAKE_TIMEOUT,
    };
    use crate::native_shadow_qualification::NativeShadowQualificationReadiness;

    const SOCKET_PATH: &str = "/run/boole/native-shadow/launcher.sock";

    #[derive(Debug)]
    pub(super) struct InstalledLauncherTransport {
        readiness: NativeShadowQualificationReadiness,
    }

    struct DeadlineUnixStream {
        stream: UnixStream,
        deadline: Instant,
    }

    impl DeadlineUnixStream {
        fn new(stream: UnixStream, deadline: Instant) -> Self {
            Self { stream, deadline }
        }

        fn set_deadline(&mut self, deadline: Instant) {
            self.deadline = deadline;
        }

        fn remaining(&self) -> io::Result<Duration> {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "launcher phase deadline elapsed",
                ))
            } else {
                Ok(remaining)
            }
        }

        fn shutdown(&self, how: Shutdown) -> io::Result<()> {
            self.stream.shutdown(how)
        }
    }

    impl Read for DeadlineUnixStream {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.stream.set_read_timeout(Some(self.remaining()?))?;
            self.stream.read(buffer)
        }
    }

    impl Write for DeadlineUnixStream {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.stream.set_write_timeout(Some(self.remaining()?))?;
            self.stream.write(buffer)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.stream.set_write_timeout(Some(self.remaining()?))?;
            self.stream.flush()
        }
    }

    impl InstalledLauncherTransport {
        pub(super) fn new(readiness: NativeShadowQualificationReadiness) -> Self {
            Self { readiness }
        }

        fn execute_session(
            &self,
            request: &ExecutionRequest,
        ) -> Result<ValidatedLauncherOutcome, &'static str> {
            let request_frame =
                encode_execution_request_frame(request).map_err(|_| "launcher_request_encode")?;
            let hello = ExecutionHello::try_from_execution_request_frame(&request_frame)
                .map_err(|_| "launcher_hello_derive")?;
            let stream =
                connect_unix_with_timeout(Path::new(SOCKET_PATH), LAUNCHER_CONNECT_TIMEOUT)
                    .map_err(|_| "launcher_connect_failed")?;
            let peer = peer_identity(&stream).map_err(|_| "launcher_peer_credentials")?;
            if peer.pid != self.readiness.launcher_pid() || peer.uid != 0 || peer.gid != 0 {
                return Err("launcher_peer_not_qualified");
            }

            // One monotonic deadline covers every handshake read/write/flush,
            // including partial-frame trickling. It is replaced once, after
            // Ready-v3, by one cumulative execution/report/EOF deadline.
            let mut stream =
                DeadlineUnixStream::new(stream, Instant::now() + LAUNCHER_HANDSHAKE_TIMEOUT);

            write_execution_hello(&mut stream, &hello)
                .map_err(|_| "launcher_hello_write_failed")?;
            let ready = read_closed_local_replay_execution_ready(&mut stream)
                .map_err(|_| "launcher_ready_invalid")?
                .ok_or("launcher_ready_missing")?;
            validate_qualified_launcher_identity(
                self.readiness.launcher_pid(),
                self.readiness.launcher_instance_id_hex(),
                peer,
                ready.launcher_pid(),
                ready.launcher_instance_id_hex(),
            )?;

            stream.set_deadline(Instant::now() + LAUNCHER_EXECUTION_TIMEOUT);
            write_execution_request(&mut stream, request)
                .map_err(|_| "launcher_execute_write_failed")?;
            stream
                .shutdown(Shutdown::Write)
                .map_err(|_| "launcher_shutdown_write_failed")?;
            let report = read_execution_report(&mut stream)
                .map_err(|_| "launcher_report_invalid")?
                .ok_or("launcher_report_missing")?;
            if read_execution_report(&mut stream)
                .map_err(|_| "launcher_terminal_eof_invalid")?
                .is_some()
            {
                return Err("launcher_second_report");
            }
            let session = validate_closed_local_replay_execution_session(
                &hello,
                &ready,
                &request_frame,
                &report,
            )
            .map_err(|_| "launcher_session_binding_invalid")?;
            Ok(classify_validated_launcher_session(&session))
        }
    }

    impl LauncherTransport<ExecutionRequest> for InstalledLauncherTransport {
        fn execute(&self, request: &ExecutionRequest) -> ValidatedLauncherOutcome {
            self.execute_session(request)
                .unwrap_or_else(|reason_code| ValidatedLauncherOutcome::Ambiguous { reason_code })
        }
    }

    #[allow(unsafe_code)]
    fn peer_identity(stream: &UnixStream) -> io::Result<LauncherPeerIdentity> {
        let mut credentials = MaybeUninit::<libc::ucred>::uninit();
        let mut length = mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: `credentials` is writable for exactly `length` bytes and
        // `stream` owns a live Unix-domain socket descriptor.
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                credentials.as_mut_ptr().cast::<libc::c_void>(),
                &mut length,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        if length as usize != mem::size_of::<libc::ucred>() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SO_PEERCRED returned an unexpected credential length",
            ));
        }
        // SAFETY: successful `getsockopt` with the exact expected length
        // initialized the complete `libc::ucred` value.
        let credentials = unsafe { credentials.assume_init() };
        Ok(LauncherPeerIdentity {
            pid: u32::try_from(credentials.pid).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "SO_PEERCRED PID is negative")
            })?,
            uid: credentials.uid,
            gid: credentials.gid,
        })
    }

    #[allow(unsafe_code)]
    fn connect_unix_with_timeout(path: &Path, timeout: Duration) -> io::Result<UnixStream> {
        // SAFETY: `socket` has no pointer arguments and returns one new FD,
        // which is immediately wrapped in `OwnedFd` below.
        let raw_fd = unsafe {
            libc::socket(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                0,
            )
        };
        if raw_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the successful `socket` call returned one uniquely owned FD.
        let descriptor = unsafe { OwnedFd::from_raw_fd(raw_fd) };
        let (address, address_length) = unix_address(path)?;
        // SAFETY: `address` is initialized for `address_length` bytes and the
        // descriptor is a live AF_UNIX stream socket.
        let connected = unsafe {
            libc::connect(
                descriptor.as_raw_fd(),
                (&address as *const libc::sockaddr_un).cast::<libc::sockaddr>(),
                address_length,
            )
        };
        if connected != 0 {
            let error = io::Error::last_os_error();
            if !matches!(
                error.raw_os_error(),
                Some(code) if code == libc::EINPROGRESS || code == libc::EAGAIN
            ) {
                return Err(error);
            }
            wait_until_connected(descriptor.as_raw_fd(), timeout)?;
        }
        set_nonblocking(descriptor.as_raw_fd(), false)?;
        Ok(UnixStream::from(descriptor))
    }

    #[allow(unsafe_code)]
    fn unix_address(path: &Path) -> io::Result<(libc::sockaddr_un, libc::socklen_t)> {
        use std::os::unix::ffi::OsStrExt;

        let path = path.as_os_str().as_bytes();
        // SAFETY: all-zero is a valid initial state for `sockaddr_un` before
        // its family and pathname bytes are filled below.
        let mut address = unsafe { mem::zeroed::<libc::sockaddr_un>() };
        if path.contains(&0) || path.len() + 1 > address.sun_path.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unix socket path is invalid or too long",
            ));
        }
        address.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (destination, source) in address.sun_path.iter_mut().zip(path.iter().copied()) {
            *destination = source as libc::c_char;
        }
        let length = mem::offset_of!(libc::sockaddr_un, sun_path) + path.len() + 1;
        Ok((address, length as libc::socklen_t))
    }

    #[allow(unsafe_code)]
    fn wait_until_connected(fd: libc::c_int, timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "launcher socket connection timed out",
                ));
            }
            let mut descriptor = libc::pollfd {
                fd,
                events: libc::POLLOUT,
                revents: 0,
            };
            // SAFETY: `descriptor` points to one initialized `pollfd` for the
            // duration of the call.
            let result = unsafe {
                libc::poll(
                    &mut descriptor,
                    1,
                    remaining.as_millis().clamp(1, i32::MAX as u128) as i32,
                )
            };
            if result == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "launcher socket connection timed out",
                ));
            }
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            let mut socket_error = 0;
            let mut length = mem::size_of::<libc::c_int>() as libc::socklen_t;
            // SAFETY: `socket_error` is writable storage of exactly `length`
            // bytes and `fd` is still the owned socket descriptor.
            if unsafe {
                libc::getsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_ERROR,
                    (&mut socket_error as *mut libc::c_int).cast::<libc::c_void>(),
                    &mut length,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            return if socket_error == 0 {
                Ok(())
            } else {
                Err(io::Error::from_raw_os_error(socket_error))
            };
        }
    }

    #[allow(unsafe_code)]
    fn set_nonblocking(fd: libc::c_int, enabled: bool) -> io::Result<()> {
        // SAFETY: `fd` is live and F_GETFL has no pointer argument.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        let flags = if enabled {
            flags | libc::O_NONBLOCK
        } else {
            flags & !libc::O_NONBLOCK
        };
        // SAFETY: `fd` is live and `flags` came from F_GETFL with only the
        // O_NONBLOCK bit adjusted.
        if unsafe { libc::fcntl(fd, libc::F_SETFL, flags) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Read;
        use std::thread;

        #[test]
        fn partial_frame_trickling_cannot_reset_the_absolute_deadline() {
            let (reader, mut writer) = UnixStream::pair().expect("Unix stream pair");
            let writer_thread = thread::spawn(move || {
                for byte in 0_u8..5 {
                    writer.write_all(&[byte]).expect("write one byte");
                    thread::sleep(Duration::from_millis(20));
                }
            });
            let started = Instant::now();
            let mut reader = DeadlineUnixStream::new(reader, started + Duration::from_millis(35));
            let mut bytes = [0_u8; 5];
            assert!(reader.read_exact(&mut bytes).is_err());
            assert!(
                started.elapsed() < Duration::from_millis(75),
                "the whole phase, not each partial read, owns one deadline"
            );
            writer_thread.join().expect("writer thread");
        }
    }
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn linux_fresh_nonce_hex() -> std::io::Result<String> {
    let mut bytes = [0_u8; 32];
    // SAFETY: `bytes` is writable for exactly its live 32-byte extent. The
    // kernel neither retains the pointer nor aliases a Rust reference.
    let actual =
        unsafe { libc::getrandom(bytes.as_mut_ptr().cast::<libc::c_void>(), bytes.len(), 0) };
    if actual < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if actual as usize != bytes.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            format!("getrandom returned {actual} bytes instead of 32"),
        ));
    }
    Ok(hex::encode(bytes))
}

struct ReplayJournalState {
    authority: NativeShadowJournalAuthority,
    store: NativeShadowStateStore,
    exhaustion: NativeShadowExhaustionLedger,
    attempts: crate::native_shadow::NativeShadowGrantAttemptLedgerV1,
}

struct ClosedLocalReplayService<A, L> {
    replay_authority: A,
    launcher: L,
    execution_gate: Arc<NativeShadowExecutionGate>,
    journal: Mutex<ReplayJournalState>,
    poisoned: Arc<AtomicBool>,
}

struct InFlightPoisonGuard {
    poisoned: Arc<AtomicBool>,
    armed: bool,
}

impl InFlightPoisonGuard {
    fn new(poisoned: Arc<AtomicBool>) -> Self {
        Self {
            poisoned,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for InFlightPoisonGuard {
    fn drop(&mut self) {
        if self.armed {
            self.poisoned.store(true, Ordering::Release);
        }
    }
}

async fn body_only_deadline<F, T>(future: F, timeout: Duration) -> Result<T, BodyReadFailure>
where
    F: Future<Output = Result<T, BodyReadFailure>>,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(result) => result,
        Err(_) => Err(BodyReadFailure::Timeout),
    }
}

async fn read_submission_body(body: Body, timeout: Duration) -> Result<Bytes, BodyReadFailure> {
    body_only_deadline(
        async move {
            to_bytes(body, HTTP_BODY_LIMIT_BYTES)
                .await
                .map_err(|_| BodyReadFailure::TooLargeOrUnreadable)
        },
        timeout,
    )
    .await
}

async fn await_owner_response<T>(
    receiver: tokio::sync::oneshot::Receiver<T>,
    timeout: Duration,
) -> Result<T, OwnerResponseFailure> {
    match tokio::time::timeout(timeout, receiver).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => Err(OwnerResponseFailure::OwnerDropped),
        Err(_) => Err(OwnerResponseFailure::Deadline),
    }
}

fn has_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"))
}

fn fixed_http_listener_address() -> std::net::SocketAddr {
    std::net::SocketAddr::V4(std::net::SocketAddrV4::new(
        std::net::Ipv4Addr::LOCALHOST,
        FIXED_HTTP_LISTENER_PORT,
    ))
}

fn ensure_no_unresolved_in_flight(stuck_in_flight: &[NativeShadowFourTuple]) -> anyhow::Result<()> {
    anyhow::ensure!(
        stuck_in_flight.is_empty(),
        "native-shadow replay route remains closed while durable InFlight rows are unresolved"
    );
    Ok(())
}

fn qualify_after_recovery_gate<T>(
    stuck_in_flight: &[NativeShadowFourTuple],
    qualify: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    ensure_no_unresolved_in_flight(stuck_in_flight)?;
    qualify()
}

fn json_response(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

fn error_response(
    status: StatusCode,
    outcome: &'static str,
    reason_code: &'static str,
) -> Response {
    json_response(
        status,
        json!({
            "schema": "boole.native-shadow.adjudication-error.v1",
            "outcome": outcome,
            "reasonCode": reason_code,
        }),
    )
}

fn adjudication_unknown_response(reason_code: &'static str) -> Response {
    json_response(
        StatusCode::GATEWAY_TIMEOUT,
        json!({
            "schema": "boole.native-shadow.adjudication-unknown.v1",
            "outcome": "adjudication_unknown",
            "reasonCode": reason_code,
            "retryAuthorized": false,
        }),
    )
}

fn adjudication_response(
    receipt: &boole_core::useful_product::VerificationReceipt,
    outcome: &str,
    reason_code: &str,
    evidence_digest: &str,
    redelivered: bool,
) -> Response {
    json_response(
        StatusCode::OK,
        json!({
            "schema": "boole.native-shadow.adjudication.v1",
            "outcome": outcome,
            "reasonCode": reason_code,
            "redelivered": redelivered,
            "evidenceDigest": evidence_digest,
            "receipt": {
                "taskId": receipt.task_id.to_hex(),
                "submissionId": receipt.submission_id.to_hex(),
                "artifactRoot": receipt.artifact_root.to_hex(),
                "checkerHash": receipt.checker_hash.to_hex(),
                "verdict": if receipt.accepted() { "accepted" } else { "rejected" },
                "rejectReason": receipt.reject_label(),
            }
        }),
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(bytes);
    hex::encode(digest.finalize())
}

fn build_router<A, L>(service: Arc<ClosedLocalReplayService<A, L>>) -> Router
where
    A: ReplayAuthority,
    L: LauncherTransport<A::Request>,
{
    Router::new()
        .route(SUBMISSION_ROUTE, post(submit::<A, L>))
        .with_state(service)
}

async fn submit<A, L>(
    State(service): State<Arc<ClosedLocalReplayService<A, L>>>,
    request: Request<Body>,
) -> Response
where
    A: ReplayAuthority,
    L: LauncherTransport<A::Request>,
{
    if !has_json_content_type(request.headers()) {
        return error_response(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "precheck_reject",
            "content_type_not_json",
        );
    }
    if service.poisoned.load(Ordering::Acquire) {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "retryable_unavailable",
            "native_service_poisoned",
        );
    }
    let body = match read_submission_body(request.into_body(), HTTP_BODY_TIMEOUT).await {
        Ok(body) => body,
        Err(BodyReadFailure::Timeout) => {
            return error_response(
                StatusCode::REQUEST_TIMEOUT,
                "precheck_reject",
                "body_timeout",
            )
        }
        Err(BodyReadFailure::TooLargeOrUnreadable) => {
            return error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "precheck_reject",
                "body_too_large_or_unreadable",
            )
        }
    };
    let submission = match NativeShadowSubmission::parse_strict(&body) {
        Ok(submission) => submission,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "precheck_reject",
                "malformed_input",
            )
        }
    };

    let redelivery_identity = match service.replay_authority.redelivery_identity(&submission) {
        Ok(identity) => identity,
        Err(_) => return poison_response(&service),
    };
    match terminal_redelivery_response(&service, &redelivery_identity) {
        Ok(Some(response)) => return response,
        Ok(None) => {}
        Err(()) => return poison_response(&service),
    }
    match pre_intake_state_response(&service, &redelivery_identity) {
        Ok(Some(response)) => return response,
        Ok(None) => {}
        Err(()) => return poison_response(&service),
    }

    let work = match submission.extract_submission_source() {
        Ok(source) => match service
            .replay_authority
            .prepare_checker(&submission, &source)
        {
            Ok(prepared) => OwnerWork::Checker(prepared),
            Err(_) => {
                return error_response(StatusCode::NOT_FOUND, "precheck_reject", "unknown_identity")
            }
        },
        Err(NativeShadowIntakeError::EmptyResponse) => {
            match service.replay_authority.prepare_pre_intake(&submission) {
                Ok(prepared) => OwnerWork::PreIntake(prepared),
                Err(_) => {
                    return error_response(
                        StatusCode::NOT_FOUND,
                        "precheck_reject",
                        "unknown_identity",
                    )
                }
            }
        }
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "precheck_reject",
                "intake_rejected",
            )
        }
    };

    // This detached owner, rather than the request future, owns every mutable
    // capability. Client cancellation can drop only the oneshot receiver; it
    // cannot release the execution permit, abort spawn_blocking, or skip the
    // durable terminal/rollback transition.
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let owner_service = Arc::clone(&service);
    tokio::spawn(async move {
        let response = match work {
            OwnerWork::Checker(prepared) => process_checker(owner_service, prepared).await,
            OwnerWork::PreIntake(prepared) => process_pre_intake(owner_service, prepared),
        };
        let _ = sender.send(response);
    });
    match await_owner_response(receiver, HTTP_TOTAL_RESPONSE_TIMEOUT).await {
        Ok(response) => response,
        Err(OwnerResponseFailure::Deadline) => {
            service.poisoned.store(true, Ordering::Release);
            // The detached owner may already be inside an uninterruptible
            // journal fsync and can still reach one durable terminal result.
            // Never promise retryability while that fact is unknowable.
            adjudication_unknown_response("owner_deadline_exceeded")
        }
        Err(OwnerResponseFailure::OwnerDropped) => {
            service.poisoned.store(true, Ordering::Release);
            adjudication_unknown_response("owner_result_unavailable")
        }
    }
}

enum OwnerWork<P, E, R> {
    Checker(PreparedCheckerCase<P, R>),
    PreIntake(PreparedPreIntakeCase<E>),
}

fn terminal_redelivery_response<A, L>(
    service: &Arc<ClosedLocalReplayService<A, L>>,
    identity: &ReplayRedeliveryIdentity,
) -> Result<Option<Response>, ()>
where
    A: ReplayAuthority,
    L: LauncherTransport<A::Request>,
{
    let journal = service.journal.lock().map_err(|_| ())?;
    let Some(evidence) = journal
        .store
        .terminal_durable_evidence(&identity.four_tuple)
    else {
        return Ok(None);
    };
    if evidence.candidate_digest != identity.candidate_digest
        || evidence.submission_digest != identity.submission_digest
    {
        return Ok(None);
    }
    let evidence_digest = journal
        .store
        .terminal_durable_evidence_digest(&identity.four_tuple)
        .ok_or(())?;
    let receipt = journal
        .store
        .map_terminal_v2_to_bf3_receipt(
            &identity.four_tuple,
            NativeShadowTerminalReceiptBinding {
                registry_version: &identity.registry_version,
                registry_digest: &identity.registry_digest,
                execution_policy_digest: &identity.execution_policy_digest,
                candidate_digest: &identity.candidate_digest,
                submission_digest: &identity.submission_digest,
            },
            &journal.exhaustion,
        )
        .map_err(|_| ())?;
    let outcome = match evidence.verdict {
        NativeShadowEvidenceVerdict::Accepted => "accepted",
        NativeShadowEvidenceVerdict::DeterministicReject => "deterministic_reject",
    };
    Ok(Some(adjudication_response(
        &receipt,
        outcome,
        &evidence.reason_code,
        evidence_digest,
        true,
    )))
}

/// Apply the durable challenge-state boundary before family intake. Exact
/// terminal redelivery is handled immediately above; a different candidate
/// for the same consumed challenge therefore reaches `challenge_exhausted`
/// without the parser/compiler seeing its answer bytes.
fn pre_intake_state_response<A, L>(
    service: &Arc<ClosedLocalReplayService<A, L>>,
    identity: &ReplayRedeliveryIdentity,
) -> Result<Option<Response>, ()>
where
    A: ReplayAuthority,
    L: LauncherTransport<A::Request>,
{
    let journal = service.journal.lock().map_err(|_| ())?;
    let view = match journal.store.admission_view_with_execution_policy(
        &identity.four_tuple,
        &identity.registry_digest,
        &identity.execution_policy_digest,
        &journal.exhaustion,
    ) {
        Ok(view) => view,
        Err(NativeShadowAdmissionError::NoSuchRow(_)) => return Ok(None),
        Err(NativeShadowAdmissionError::RegistryDrift { .. }) => {
            return Ok(Some(error_response(
                StatusCode::CONFLICT,
                "precheck_reject",
                "registry_drift",
            )))
        }
        Err(NativeShadowAdmissionError::ExecutionPolicyDrift { .. }) => {
            return Ok(Some(error_response(
                StatusCode::CONFLICT,
                "precheck_reject",
                "execution_policy_drift",
            )))
        }
        Err(NativeShadowAdmissionError::TerminalProjectionMismatch { .. }) => return Err(()),
    };
    Ok(match view {
        NativeShadowAdmissionView::ActiveFresh => None,
        NativeShadowAdmissionView::InFlight => Some(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "retryable_unavailable",
            "native_busy",
        )),
        NativeShadowAdmissionView::ChallengeExhausted => Some(error_response(
            StatusCode::CONFLICT,
            "precheck_reject",
            "challenge_exhausted",
        )),
        NativeShadowAdmissionView::ChallengeDisabled => Some(error_response(
            StatusCode::CONFLICT,
            "precheck_reject",
            "challenge_disabled",
        )),
        NativeShadowAdmissionView::ChallengeStale => Some(error_response(
            StatusCode::CONFLICT,
            "precheck_reject",
            "challenge_stale",
        )),
    })
}

fn process_pre_intake<A, L>(
    service: Arc<ClosedLocalReplayService<A, L>>,
    prepared: PreparedPreIntakeCase<A::PreIntakePrepared>,
) -> Response
where
    A: ReplayAuthority,
    L: LauncherTransport<A::Request>,
{
    if service.poisoned.load(Ordering::Acquire) {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "retryable_unavailable",
            "native_service_poisoned",
        );
    }
    if service
        .replay_authority
        .validate_pre_intake_before_reserve(&prepared)
        .is_err()
    {
        return poison_response(&service);
    }
    let PreparedPreIntakeCase {
        prepared,
        four_tuple,
        registry_digest,
        execution_policy_digest,
        operation_id_hex,
        candidate_digest,
        submission_digest,
    } = prepared;
    let reservation = {
        let mut journal = match service.journal.lock() {
            Ok(journal) => journal,
            Err(_) => return poison_response(&service),
        };
        let ReplayJournalState {
            authority,
            attempts,
            ..
        } = &mut *journal;
        attempts.reserve(
            authority,
            NativeShadowGrantAttemptFieldsV1 {
                four_tuple: &four_tuple,
                registry_digest: &registry_digest,
                execution_policy_digest: &execution_policy_digest,
                operation_id_hex: &operation_id_hex,
                candidate_digest: &candidate_digest,
                submission_digest: &submission_digest,
                kind: NativeShadowGrantAttemptKindV1::PreIntake,
            },
        )
    };
    let _reservation = match reservation {
        Ok(reservation) => reservation,
        Err(error)
            if classify_attempt_reservation_error(&error)
                == AttemptReservationDisposition::ChallengeExhausted =>
        {
            return error_response(
                StatusCode::CONFLICT,
                "precheck_reject",
                "challenge_exhausted",
            )
        }
        Err(_) => return poison_unknown_response(&service, "attempt_reservation_outcome_unknown"),
    };
    if service
        .replay_authority
        .authorize_pre_intake(prepared)
        .is_err()
    {
        return poison_unknown_response(&service, "post_reservation_authorization_failed");
    }
    error_response(
        StatusCode::BAD_REQUEST,
        "precheck_reject",
        "intake_rejected",
    )
}

async fn process_checker<A, L>(
    service: Arc<ClosedLocalReplayService<A, L>>,
    prepared: PreparedCheckerCase<A::CheckerPrepared, A::Request>,
) -> Response
where
    A: ReplayAuthority,
    L: LauncherTransport<A::Request>,
{
    if service.poisoned.load(Ordering::Acquire) {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "retryable_unavailable",
            "native_service_poisoned",
        );
    }
    let _permit = match service.execution_gate.try_acquire() {
        Ok(permit) => permit,
        Err(busy) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "retryable_unavailable",
                busy.reason_code(),
            )
        }
    };

    if service
        .replay_authority
        .validate_checker_before_reserve(&prepared)
        .is_err()
    {
        return poison_response(&service);
    }

    let reservation = {
        let mut journal = match service.journal.lock() {
            Ok(journal) => journal,
            Err(_) => return poison_response(&service),
        };
        let ReplayJournalState {
            authority,
            attempts,
            ..
        } = &mut *journal;
        attempts.reserve(
            authority,
            NativeShadowGrantAttemptFieldsV1 {
                four_tuple: &prepared.identity.four_tuple,
                registry_digest: &prepared.identity.registry_digest,
                execution_policy_digest: &prepared.identity.execution_policy_digest,
                operation_id_hex: &prepared.identity.operation_id_hex,
                candidate_digest: &prepared.identity.candidate_digest,
                submission_digest: &prepared.identity.submission_digest,
                kind: NativeShadowGrantAttemptKindV1::Checker,
            },
        )
    };
    let reservation = match reservation {
        Ok(reservation) => reservation,
        Err(error)
            if classify_attempt_reservation_error(&error)
                == AttemptReservationDisposition::ChallengeExhausted =>
        {
            return error_response(
                StatusCode::CONFLICT,
                "precheck_reject",
                "challenge_exhausted",
            )
        }
        Err(_) => return poison_unknown_response(&service, "attempt_reservation_outcome_unknown"),
    };
    let authorized = match service.replay_authority.authorize_checker(prepared) {
        Ok(authorized) => authorized,
        Err(_) => {
            return poison_unknown_response(&service, "post_reservation_authorization_failed")
        }
    };
    let flight = {
        let mut journal = match service.journal.lock() {
            Ok(journal) => journal,
            Err(_) => return poison_unknown_response(&service, "reserved_journal_unavailable"),
        };
        let resolved = journal
            .store
            .resolve_verified_closed_local_replay(&authorized.bootstrap);
        if !matches!(
            resolved,
            ResolveOutcome::Bootstrapped(ChallengeState::ActiveFresh)
                | ResolveOutcome::Existing(ChallengeState::ActiveFresh)
        ) {
            return poison_unknown_response(&service, "reserved_state_resolution_failed");
        }
        if !matches!(
            journal.store.admission_view_with_execution_policy(
                &authorized.identity.four_tuple,
                &authorized.identity.registry_digest,
                &authorized.identity.execution_policy_digest,
                &journal.exhaustion,
            ),
            Ok(NativeShadowAdmissionView::ActiveFresh)
        ) {
            return poison_unknown_response(&service, "reserved_admission_drift");
        }
        let ReplayJournalState {
            authority, store, ..
        } = &mut *journal;
        match store.begin_reserved_closed_local_replay_execution_v3(authority, reservation) {
            Ok(flight) => flight,
            Err(_) => return poison_unknown_response(&service, "begin_in_flight_outcome_unknown"),
        }
    };
    let mut poison_guard = InFlightPoisonGuard::new(Arc::clone(&service.poisoned));

    let execution_service = Arc::clone(&service);
    let request = authorized.request;
    let launcher_outcome =
        match tokio::task::spawn_blocking(move || execution_service.launcher.execute(&request))
            .await
        {
            Ok(outcome) => outcome,
            Err(_) => ValidatedLauncherOutcome::Ambiguous {
                reason_code: "launcher_task_join_failed",
            },
        };

    let terminal = match launcher_outcome {
        ValidatedLauncherOutcome::Terminal(terminal) => terminal,
        ValidatedLauncherOutcome::RetryableClean { reason } => {
            let mut journal = match service.journal.lock() {
                Ok(journal) => journal,
                Err(_) => return poison_unknown_response(&service, "rollback_journal_unavailable"),
            };
            let ReplayJournalState {
                authority, store, ..
            } = &mut *journal;
            if store
                .retryable_rollback_v3(authority, &flight, reason)
                .is_err()
            {
                return poison_unknown_response(&service, "rollback_outcome_unknown");
            }
            poison_guard.disarm();
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "retryable_unavailable",
                reason.as_str(),
            );
        }
        ValidatedLauncherOutcome::Ambiguous { reason_code } => {
            service.poisoned.store(true, Ordering::Release);
            return adjudication_unknown_response(reason_code);
        }
    };

    let (verdict, outcome) = match terminal.verdict {
        ValidatedLauncherVerdict::Accepted => (NativeShadowEvidenceVerdict::Accepted, "accepted"),
        ValidatedLauncherVerdict::DeterministicReject => (
            NativeShadowEvidenceVerdict::DeterministicReject,
            "deterministic_reject",
        ),
    };
    let identity = authorized.identity;
    let evidence = NativeShadowEvidence {
        schema: "boole.native-shadow.evidence.v2".to_string(),
        submission_schema: "boole.native-shadow.submission.v1".to_string(),
        submission_digest: identity.submission_digest.clone(),
        family_version: identity.four_tuple.family_version.clone(),
        template_id: identity.four_tuple.template_id.clone(),
        anchor_digest: identity.anchor_digest.clone(),
        challenge_sha256: identity.four_tuple.challenge_sha256.clone(),
        epoch: identity.four_tuple.epoch,
        candidate_digest: identity.candidate_digest.clone(),
        intake_version: identity.intake_version.clone(),
        checker_digest: identity.checker_digest.clone(),
        policy_digest: identity.checker_policy_digest.clone(),
        execution_policy_digest: Some(identity.execution_policy_digest.clone()),
        toolchain_digest: identity.toolchain_digest.clone(),
        verdict,
        reason_code: terminal.reason_code.to_string(),
        registry_version: identity.registry_version.clone(),
    };
    let evidence_json = serde_json::to_string(&evidence).expect("typed evidence serializes");
    let evidence_digest = sha256_hex(evidence_json.as_bytes());
    let receipt = {
        let mut journal = match service.journal.lock() {
            Ok(journal) => journal,
            Err(_) => return poison_unknown_response(&service, "terminal_journal_unavailable"),
        };
        let ReplayJournalState {
            authority,
            store,
            exhaustion,
            ..
        } = &mut *journal;
        let durable = match store.persist_evidence(
            authority,
            &identity.four_tuple,
            &identity.candidate_digest,
            &evidence_json,
        ) {
            Ok(durable) => durable,
            Err(_) => return poison_unknown_response(&service, "evidence_commit_outcome_unknown"),
        };
        let receipt = match store.map_durable_v2_to_bf3_receipt(&identity.four_tuple, &durable) {
            Ok(receipt) => receipt,
            Err(_) => return poison_unknown_response(&service, "receipt_mapping_failed"),
        };
        if store
            .complete_consumed(authority, exhaustion, &identity.four_tuple, durable)
            .is_err()
        {
            return poison_unknown_response(&service, "terminal_commit_outcome_unknown");
        }
        receipt
    };
    poison_guard.disarm();
    adjudication_response(
        &receipt,
        outcome,
        terminal.reason_code,
        &evidence_digest,
        false,
    )
}

fn poison_response<A, L>(service: &Arc<ClosedLocalReplayService<A, L>>) -> Response {
    service.poisoned.store(true, Ordering::Release);
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "retryable_unavailable",
        "native_service_poisoned",
    )
}

fn poison_unknown_response<A, L>(
    service: &Arc<ClosedLocalReplayService<A, L>>,
    reason_code: &'static str,
) -> Response {
    service.poisoned.store(true, Ordering::Release);
    adjudication_unknown_response(reason_code)
}

#[cfg(target_os = "linux")]
pub async fn serve_installed_closed_local_native_shadow_replay() -> anyhow::Result<()> {
    use boole_native_shadow_protocol::installed_authority::open_verified_installed_closed_local_replay_execution_authorities;
    use boole_native_shadow_protocol::resolve_fixed_service_identities;

    let identities = resolve_fixed_service_identities()?;
    let installed = open_verified_installed_closed_local_replay_execution_authorities()?;
    anyhow::ensure!(
        installed.grant().loopback_only()
            && !installed.grant().p2p_allowed()
            && !installed.grant().consensus_allowed()
            && !installed.grant().reward_allowed()
            && !installed.grant().mineable_now()
            && !installed.grant().activation_allowed()
            && installed.grant().non_issuable(),
        "closed-local replay node refuses any issuance, P2P, consensus, reward or activation authority"
    );
    let registry_version = installed.grant().registry_version().to_string();
    let registry_digest = installed.grant().registry_digest_hex().to_string();
    let production_registry_digest = installed
        .grant()
        .production_registry_digest_hex()
        .to_string();
    let execution_policy_digest =
        crate::native_shadow::NativeShadowExecutionPolicyDigest::try_from(
            installed
                .execution_authority()
                .base_execution_policy_sha256(),
        )
        .map_err(anyhow::Error::msg)?;

    // The lifetime-held journal lock is acquired before launcher
    // qualification, so the route cannot race another node process while the
    // authenticated zero-leaf barrier is being established.
    let mut journal_authority = NativeShadowJournalAuthority::open_production(
        identities.node_uid(),
        identities.node_gid(),
    )?;
    let recovery = crate::native_shadow::recover_verified_closed_local_replay_state(
        &registry_version,
        &registry_digest,
        &execution_policy_digest,
        &mut journal_authority,
    )?;
    recovery
        .attempts
        .validate_against_closed_local_grant(installed.grant(), &execution_policy_digest)?;

    // Recovery already tells us whether an earlier execution is unresolved.
    // The current grant intentionally cannot mutate that recovery state, so
    // refuse it before starting the comparatively expensive launcher/rootfs
    // qualification path, which is especially visible on native ARM runners.
    // No route or checker process is needed to make this fail-closed decision.
    let readiness = qualify_after_recovery_gate(&recovery.stuck_in_flight, || {
        crate::native_shadow_qualification::qualify_installed_native_shadow_launcher()
    })?;
    anyhow::ensure!(
        readiness.registry_digest_hex() == production_registry_digest,
        "qualified launcher registry digest differs from the replay grant's production registry"
    );
    anyhow::ensure!(
        readiness.execution_policy_digest_hex() == execution_policy_digest.as_str(),
        "qualified launcher execution-policy digest differs from the replay authority"
    );
    anyhow::ensure!(
        readiness.toolchain_identity_digest_hex()
            == sha256_hex(boole_native_shadow_protocol::TRACKED_TOOLCHAIN_IDENTITY_BYTES),
        "qualified launcher toolchain digest differs from the installed replay authority"
    );
    let launcher = installed_launcher_transport::InstalledLauncherTransport::new(readiness);
    let service = Arc::new(ClosedLocalReplayService {
        replay_authority: InstalledReplayAuthority::new(installed),
        launcher,
        execution_gate: Arc::new(NativeShadowExecutionGate::new()),
        journal: Mutex::new(ReplayJournalState {
            authority: journal_authority,
            store: recovery.store,
            exhaustion: recovery.exhaustion_ledger,
            attempts: recovery.attempts,
        }),
        poisoned: Arc::new(AtomicBool::new(false)),
    });
    let listener = tokio::net::TcpListener::bind(fixed_http_listener_address()).await?;
    axum::serve(listener, build_router(service)).await?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub async fn serve_installed_closed_local_native_shadow_replay() -> anyhow::Result<()> {
    anyhow::bail!("closed-local native-shadow replay requires Linux")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    use axum::http::Method;
    use boole_testkit::rand_suffix;
    use tower::ServiceExt;

    use crate::native_shadow::NativeShadowExecutionPolicyDigest;

    #[derive(Debug)]
    struct TestRequest {
        candidate_digest: String,
    }

    #[derive(Debug)]
    struct TestAuthority {
        prepare_calls: AtomicUsize,
        allow_prepare: AtomicBool,
        allow_checker_validation: AtomicBool,
        allow_pre_intake_validation: AtomicBool,
        allow_checker_authorization: AtomicBool,
        allow_pre_intake_authorization: AtomicBool,
    }

    impl Default for TestAuthority {
        fn default() -> Self {
            Self {
                prepare_calls: AtomicUsize::new(0),
                allow_prepare: AtomicBool::new(true),
                allow_checker_validation: AtomicBool::new(true),
                allow_pre_intake_validation: AtomicBool::new(true),
                allow_checker_authorization: AtomicBool::new(true),
                allow_pre_intake_authorization: AtomicBool::new(true),
            }
        }
    }

    impl TestAuthority {
        fn identity(submission: &NativeShadowSubmission) -> ReplayCaseIdentity {
            ReplayCaseIdentity {
                four_tuple: NativeShadowFourTuple {
                    family_version: submission.family_version().to_string(),
                    template_id: submission.template_id().to_string(),
                    challenge_sha256: submission.challenge_sha256().to_string(),
                    epoch: submission.epoch(),
                },
                registry_version: "closed-local-test-v1".to_string(),
                registry_digest: digest('d'),
                execution_policy_digest: NativeShadowExecutionPolicyDigest::try_from(
                    digest('e').as_str(),
                )
                .expect("policy digest"),
                operation_id_hex: sha256_hex(
                    format!("operation:{}", submission.submission_digest_hex()).as_bytes(),
                ),
                candidate_digest: submission.candidate_digest_hex(),
                submission_digest: submission.submission_digest_hex(),
                anchor_digest: digest('a'),
                intake_version: "RUST-TUPLE-STRUCT-NATIVE-PROOF-INTAKE-V1".to_string(),
                checker_digest: digest('b'),
                checker_policy_digest: digest('c'),
                toolchain_digest: digest('f'),
            }
        }
    }

    impl ReplayAuthority for TestAuthority {
        type CheckerPrepared = ();
        type PreIntakePrepared = ();
        type Request = TestRequest;

        fn redelivery_identity(
            &self,
            submission: &NativeShadowSubmission,
        ) -> Result<ReplayRedeliveryIdentity, &'static str> {
            Ok(ReplayRedeliveryIdentity::from(&Self::identity(submission)))
        }

        fn prepare_checker(
            &self,
            submission: &NativeShadowSubmission,
            _source: &NativeShadowSubmissionSource,
        ) -> Result<PreparedCheckerCase<Self::CheckerPrepared, Self::Request>, &'static str>
        {
            self.prepare_calls.fetch_add(1, AtomicOrdering::SeqCst);
            if !self.allow_prepare.load(Ordering::Acquire) {
                return Err("request_preparation_disabled");
            }
            let identity = Self::identity(submission);
            Ok(PreparedCheckerCase {
                prepared: (),
                request: TestRequest {
                    candidate_digest: identity.candidate_digest.clone(),
                },
                identity,
            })
        }

        fn authorize_checker(
            &self,
            prepared: PreparedCheckerCase<Self::CheckerPrepared, Self::Request>,
        ) -> Result<AuthorizedCheckerCase<Self::Request>, &'static str> {
            if !self.allow_checker_authorization.load(Ordering::Acquire) {
                return Err("checker_authorization_disabled");
            }
            let () = prepared.prepared;
            let bootstrap = VerifiedNativeShadowReplayBootstrap::for_test(
                prepared.identity.four_tuple.clone(),
                &prepared.identity.registry_version,
                prepared.identity.registry_digest.clone(),
                prepared.identity.execution_policy_digest.clone(),
            );
            Ok(AuthorizedCheckerCase {
                request: prepared.request,
                identity: prepared.identity,
                bootstrap,
            })
        }

        fn validate_checker_before_reserve(
            &self,
            _prepared: &PreparedCheckerCase<Self::CheckerPrepared, Self::Request>,
        ) -> Result<(), &'static str> {
            self.allow_checker_validation
                .load(Ordering::Acquire)
                .then_some(())
                .ok_or("checker_validation_disabled")
        }

        fn prepare_pre_intake(
            &self,
            submission: &NativeShadowSubmission,
        ) -> Result<PreparedPreIntakeCase<Self::PreIntakePrepared>, &'static str> {
            let identity = Self::identity(submission);
            Ok(PreparedPreIntakeCase {
                prepared: (),
                four_tuple: identity.four_tuple,
                registry_digest: identity.registry_digest,
                execution_policy_digest: identity.execution_policy_digest,
                operation_id_hex: identity.operation_id_hex,
                candidate_digest: identity.candidate_digest,
                submission_digest: identity.submission_digest,
            })
        }

        fn authorize_pre_intake(
            &self,
            _prepared: Self::PreIntakePrepared,
        ) -> Result<(), &'static str> {
            self.allow_pre_intake_authorization
                .load(Ordering::Acquire)
                .then_some(())
                .ok_or("pre_intake_authorization_disabled")
        }

        fn validate_pre_intake_before_reserve(
            &self,
            _prepared: &PreparedPreIntakeCase<Self::PreIntakePrepared>,
        ) -> Result<(), &'static str> {
            self.allow_pre_intake_validation
                .load(Ordering::Acquire)
                .then_some(())
                .ok_or("pre_intake_validation_disabled")
        }
    }

    struct TestLauncher {
        outcomes: Mutex<VecDeque<ValidatedLauncherOutcome>>,
        calls: AtomicUsize,
        started: AtomicBool,
        delay: Duration,
    }

    impl TestLauncher {
        fn new(outcomes: impl IntoIterator<Item = ValidatedLauncherOutcome>) -> Arc<Self> {
            Arc::new(Self {
                outcomes: Mutex::new(outcomes.into_iter().collect()),
                calls: AtomicUsize::new(0),
                started: AtomicBool::new(false),
                delay: Duration::ZERO,
            })
        }

        fn delayed(
            delay: Duration,
            outcomes: impl IntoIterator<Item = ValidatedLauncherOutcome>,
        ) -> Arc<Self> {
            Arc::new(Self {
                outcomes: Mutex::new(outcomes.into_iter().collect()),
                calls: AtomicUsize::new(0),
                started: AtomicBool::new(false),
                delay,
            })
        }
    }

    impl LauncherTransport<TestRequest> for Arc<TestLauncher> {
        fn execute(&self, request: &TestRequest) -> ValidatedLauncherOutcome {
            assert_eq!(request.candidate_digest.len(), 64);
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            self.started.store(true, Ordering::Release);
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            self.outcomes
                .lock()
                .expect("outcomes")
                .pop_front()
                .expect("one frozen launcher outcome per execution")
        }
    }

    fn accepted() -> ValidatedLauncherOutcome {
        ValidatedLauncherOutcome::Terminal(ValidatedLauncherTerminal {
            verdict: ValidatedLauncherVerdict::Accepted,
            reason_code: "accepted",
        })
    }

    fn deterministic_reject() -> ValidatedLauncherOutcome {
        ValidatedLauncherOutcome::Terminal(ValidatedLauncherTerminal {
            verdict: ValidatedLauncherVerdict::DeterministicReject,
            reason_code: "compile_or_hidden_test_failed",
        })
    }

    fn digest(byte: char) -> String {
        byte.to_string().repeat(64)
    }

    fn journal_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "boole-native-shadow-route-{label}-{}-{}.ndjson",
            std::process::id(),
            rand_suffix()
        ))
    }

    fn test_service(
        label: &str,
        launcher: Arc<TestLauncher>,
    ) -> Arc<ClosedLocalReplayService<TestAuthority, Arc<TestLauncher>>> {
        let path = journal_path(label);
        let mut authority = NativeShadowJournalAuthority::open(path).expect("journal authority");
        let execution_policy_digest =
            NativeShadowExecutionPolicyDigest::try_from(digest('e').as_str())
                .expect("policy digest");
        let recovery = crate::native_shadow::recover_verified_closed_local_replay_state(
            "closed-local-test-v1",
            &digest('d'),
            &execution_policy_digest,
            &mut authority,
        )
        .expect("empty replay recovery");
        Arc::new(ClosedLocalReplayService {
            replay_authority: TestAuthority::default(),
            launcher,
            execution_gate: Arc::new(NativeShadowExecutionGate::new()),
            journal: Mutex::new(ReplayJournalState {
                authority,
                store: recovery.store,
                exhaustion: recovery.exhaustion_ledger,
                attempts: recovery.attempts,
            }),
            poisoned: Arc::new(AtomicBool::new(false)),
        })
    }

    fn submission_body(raw_answer: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "schema": "boole.native-shadow.submission.v1",
            "familyVersion": "TUPLE-STRUCT-PROJECT/RUST-TUPLE-STRUCT-PROJECT-V1",
            "templateId": digest('a'),
            "challengeSha256": digest('b'),
            "epoch": 7,
            "rawAnswer": raw_answer,
        }))
        .expect("submission JSON")
    }

    fn submission_request(raw_answer: &str) -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri(SUBMISSION_ROUTE)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(submission_body(raw_answer)))
            .expect("request")
    }

    async fn response_json(response: Response) -> Value {
        let body = to_bytes(response.into_body(), HTTP_BODY_LIMIT_BYTES)
            .await
            .expect("response body");
        serde_json::from_slice(&body).expect("response JSON")
    }

    #[tokio::test]
    async fn body_deadline_expires_without_waiting_for_adjudication_work() {
        let result = body_only_deadline(
            std::future::pending::<Result<(), BodyReadFailure>>(),
            Duration::from_millis(10),
        )
        .await;
        assert_eq!(result, Err(BodyReadFailure::Timeout));
    }

    #[tokio::test]
    async fn detached_owner_response_has_the_frozen_115_second_outer_deadline() {
        let (_sender, receiver) = tokio::sync::oneshot::channel::<Response>();
        let failure = await_owner_response(receiver, Duration::from_millis(10))
            .await
            .expect_err("a silent owner must hit the node response deadline");
        assert_eq!(failure, OwnerResponseFailure::Deadline);
        assert_eq!(HTTP_TOTAL_RESPONSE_TIMEOUT, Duration::from_secs(115));
    }

    #[tokio::test]
    async fn owner_deadline_is_reported_as_unknown_never_retryable() {
        let response = adjudication_unknown_response("owner_deadline_exceeded");
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        let body = response_json(response).await;
        assert_eq!(body["outcome"], "adjudication_unknown");
        assert_eq!(body["reasonCode"], "owner_deadline_exceeded");
        assert_eq!(body["retryAuthorized"], false);
        assert_ne!(body["outcome"], "retryable_unavailable");
    }

    #[tokio::test]
    async fn body_cap_is_absolute_and_the_listener_is_fixed_loopback_only() {
        assert!(read_submission_body(
            Body::from(vec![0_u8; HTTP_BODY_LIMIT_BYTES + 1]),
            Duration::from_secs(1),
        )
        .await
        .is_err());
        assert_eq!(
            fixed_http_listener_address(),
            "127.0.0.1:8082".parse().expect("fixed address")
        );
        assert_eq!(SUBMISSION_ROUTE, "/native-shadow/submissions");
    }

    #[test]
    fn content_type_is_json_only() {
        let mut headers = HeaderMap::new();
        assert!(!has_json_content_type(&headers));
        headers.insert(header::CONTENT_TYPE, "text/plain".parse().expect("header"));
        assert!(!has_json_content_type(&headers));
        headers.insert(
            header::CONTENT_TYPE,
            "application/json; charset=utf-8".parse().expect("header"),
        );
        assert!(has_json_content_type(&headers));
    }

    #[tokio::test]
    async fn detached_owner_rechecks_poison_before_any_durable_mutation() {
        let launcher = TestLauncher::new([accepted()]);
        let service = test_service("owner-poison-race", Arc::clone(&launcher));
        let submission =
            NativeShadowSubmission::parse_strict(&submission_body("```rust\nfn answer() {}\n```"))
                .expect("strict submission");
        let source = submission
            .extract_submission_source()
            .expect("submission source");
        let prepared = service
            .replay_authority
            .prepare_checker(&submission, &source)
            .expect("prepared checker case");

        // Model the race where this owner was detached immediately before a
        // different owner poisoned the service.
        service.poisoned.store(true, Ordering::Release);
        let response = process_checker(Arc::clone(&service), prepared).await;

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response_json(response).await["reasonCode"],
            "native_service_poisoned"
        );
        assert_eq!(launcher.calls.load(AtomicOrdering::SeqCst), 0);
        assert_eq!(
            service
                .journal
                .lock()
                .expect("journal")
                .attempts
                .total_attempts(),
            0
        );
    }

    #[tokio::test]
    async fn terminal_outcome_is_durable_and_exact_redelivery_never_relaunches() {
        let launcher = TestLauncher::new([accepted()]);
        let service = test_service("terminal-redelivery", Arc::clone(&launcher));
        let router = build_router(Arc::clone(&service));

        let first = router
            .clone()
            .oneshot(submission_request("```rust\nfn answer() {}\n```"))
            .await
            .expect("first response");
        assert_eq!(first.status(), StatusCode::OK);
        let first_json = response_json(first).await;
        assert_eq!(first_json["outcome"], "accepted");
        assert_eq!(first_json["redelivered"], false);

        service
            .replay_authority
            .allow_prepare
            .store(false, Ordering::Release);

        let second = router
            .oneshot(submission_request("```rust\nfn answer() {}\n```"))
            .await
            .expect("redelivery response");
        assert_eq!(second.status(), StatusCode::OK);
        let second_json = response_json(second).await;
        assert_eq!(second_json["outcome"], "accepted");
        assert_eq!(second_json["redelivered"], true);
        assert_eq!(second_json["receipt"], first_json["receipt"]);
        assert_eq!(second_json["evidenceDigest"], first_json["evidenceDigest"]);
        assert_eq!(launcher.calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(
            service
                .replay_authority
                .prepare_calls
                .load(AtomicOrdering::SeqCst),
            1,
            "terminal redelivery must not build a fresh launcher request"
        );
        assert_eq!(
            service
                .journal
                .lock()
                .expect("journal")
                .attempts
                .total_attempts(),
            1
        );
    }

    #[tokio::test]
    async fn empty_answer_spends_exactly_one_pre_intake_attempt() {
        let launcher = TestLauncher::new([]);
        let service = test_service("empty-once", Arc::clone(&launcher));
        let router = build_router(Arc::clone(&service));

        let first = router
            .clone()
            .oneshot(submission_request("   \n"))
            .await
            .expect("first empty response");
        assert_eq!(first.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_json(first).await["reasonCode"], "intake_rejected");

        let repeat = router
            .oneshot(submission_request("   \n"))
            .await
            .expect("repeat empty response");
        assert_eq!(repeat.status(), StatusCode::CONFLICT);
        assert_eq!(
            response_json(repeat).await["reasonCode"],
            "challenge_exhausted"
        );
        assert_eq!(launcher.calls.load(AtomicOrdering::SeqCst), 0);
        let journal = service.journal.lock().expect("journal");
        assert_eq!(journal.attempts.total_attempts(), 1);
        assert_eq!(journal.attempts.checker_attempts(), 0);
    }

    #[tokio::test]
    async fn deterministic_reject_is_also_terminal_and_redeliverable() {
        let launcher = TestLauncher::new([deterministic_reject()]);
        let service = test_service("reject-redelivery", Arc::clone(&launcher));
        let router = build_router(Arc::clone(&service));
        let answer = "```rust\nfn answer() { compile_error!(\"no\"); }\n```";

        let first = router
            .clone()
            .oneshot(submission_request(answer))
            .await
            .expect("first reject");
        assert_eq!(first.status(), StatusCode::OK);
        let first_json = response_json(first).await;
        assert_eq!(first_json["outcome"], "deterministic_reject");
        assert_eq!(first_json["receipt"]["verdict"], "rejected");

        let redelivery = router
            .oneshot(submission_request(answer))
            .await
            .expect("reject redelivery");
        assert_eq!(redelivery.status(), StatusCode::OK);
        assert_eq!(response_json(redelivery).await["redelivered"], true);
        assert_eq!(launcher.calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[tokio::test]
    async fn consumed_challenge_is_rejected_before_a_different_candidate_reaches_intake() {
        let launcher = TestLauncher::new([accepted()]);
        let service = test_service("state-before-intake", Arc::clone(&launcher));
        let router = build_router(Arc::clone(&service));

        let first = router
            .clone()
            .oneshot(submission_request("```rust\nfn answer() {}\n```"))
            .await
            .expect("first response");
        assert_eq!(first.status(), StatusCode::OK);

        let second = router
            .oneshot(submission_request("not a fenced Rust submission"))
            .await
            .expect("spent challenge response");
        assert_eq!(second.status(), StatusCode::CONFLICT);
        assert_eq!(
            response_json(second).await["reasonCode"],
            "challenge_exhausted"
        );
        assert_eq!(launcher.calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(
            service
                .replay_authority
                .prepare_calls
                .load(AtomicOrdering::SeqCst),
            1,
            "the second candidate must stop before request preparation/intake"
        );
    }

    #[tokio::test]
    async fn retryable_cleanup_rolls_back_state_but_the_frozen_case_cannot_execute_twice() {
        let launcher = TestLauncher::new([ValidatedLauncherOutcome::RetryableClean {
            reason: crate::native_shadow::NativeShadowRetryableReasonV3::CheckerInternalError,
        }]);
        let service = test_service("retryable-once", Arc::clone(&launcher));
        let router = build_router(Arc::clone(&service));
        let request_answer = "```rust\nfn answer() {}\n```";

        let first = router
            .clone()
            .oneshot(submission_request(request_answer))
            .await
            .expect("retryable response");
        assert_eq!(first.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response_json(first).await["reasonCode"],
            "checker_internal_error"
        );
        assert!(!service.poisoned.load(Ordering::Acquire));

        let repeat = router
            .oneshot(submission_request(request_answer))
            .await
            .expect("spent-attempt response");
        assert_eq!(repeat.status(), StatusCode::CONFLICT);
        assert_eq!(launcher.calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ambiguous_launcher_outcome_is_unknown_and_never_authorizes_retry() {
        let launcher = TestLauncher::new([ValidatedLauncherOutcome::Ambiguous {
            reason_code: "launcher_cleanup_incomplete",
        }]);
        let service = test_service("ambiguous-unknown", Arc::clone(&launcher));

        let response = build_router(Arc::clone(&service))
            .oneshot(submission_request("```rust\nfn answer() {}\n```"))
            .await
            .expect("ambiguous response");
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        let body = response_json(response).await;
        assert_eq!(body["outcome"], "adjudication_unknown");
        assert_eq!(body["reasonCode"], "launcher_cleanup_incomplete");
        assert_eq!(body["retryAuthorized"], false);
        assert!(service.poisoned.load(Ordering::Acquire));
        assert_eq!(launcher.calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[tokio::test]
    async fn native_busy_is_free_and_does_not_reserve_a_second_attempt() {
        let launcher = TestLauncher::delayed(Duration::from_millis(80), [accepted()]);
        let service = test_service("busy-free", Arc::clone(&launcher));
        let held_permit = service
            .execution_gate
            .try_acquire()
            .expect("hold the single slot");
        let response = build_router(Arc::clone(&service))
            .oneshot(submission_request("```rust\nfn answer() {}\n```"))
            .await
            .expect("busy response");
        drop(held_permit);

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response_json(response).await["reasonCode"], "native_busy");
        assert_eq!(launcher.calls.load(AtomicOrdering::SeqCst), 0);
        assert_eq!(
            service
                .journal
                .lock()
                .expect("journal")
                .attempts
                .total_attempts(),
            0
        );
    }

    #[tokio::test]
    async fn client_cancellation_cannot_abort_the_detached_terminal_owner() {
        let launcher = TestLauncher::delayed(Duration::from_millis(80), [accepted()]);
        let service = test_service("cancel-owner", Arc::clone(&launcher));
        let router = build_router(Arc::clone(&service));
        let request_answer = "```rust\nfn answer() {}\n```";
        let task = tokio::spawn(router.clone().oneshot(submission_request(request_answer)));
        while !launcher.started.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
        task.abort();
        let _ = task.await;
        tokio::time::sleep(Duration::from_millis(120)).await;

        let redelivery = router
            .oneshot(submission_request(request_answer))
            .await
            .expect("redelivery after client cancellation");
        assert_eq!(redelivery.status(), StatusCode::OK);
        assert_eq!(response_json(redelivery).await["redelivered"], true);
        assert_eq!(launcher.calls.load(AtomicOrdering::SeqCst), 1);
        assert!(!service.poisoned.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn malformed_request_never_mutates_the_attempt_journal() {
        let launcher = TestLauncher::new([]);
        let service = test_service("malformed-free", Arc::clone(&launcher));
        let response = build_router(Arc::clone(&service))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(SUBMISSION_ROUTE)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(b"not json".as_slice()))
                    .expect("request"),
            )
            .await
            .expect("malformed response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(launcher.calls.load(AtomicOrdering::SeqCst), 0);
        assert_eq!(
            service
                .journal
                .lock()
                .expect("journal")
                .attempts
                .total_attempts(),
            0
        );
    }

    #[tokio::test]
    async fn grant_validation_failure_never_persists_a_checker_attempt() {
        let launcher = TestLauncher::new([]);
        let service = test_service("checker-auth-before-mutation", Arc::clone(&launcher));
        service
            .replay_authority
            .allow_checker_validation
            .store(false, Ordering::Release);

        let response = build_router(Arc::clone(&service))
            .oneshot(submission_request("```rust\nfn answer() {}\n```"))
            .await
            .expect("authorization failure response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            service
                .journal
                .lock()
                .expect("journal")
                .attempts
                .total_attempts(),
            0
        );
        assert_eq!(launcher.calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[tokio::test]
    async fn post_reservation_checker_failure_is_unknown_and_never_authorizes_retry() {
        let launcher = TestLauncher::new([]);
        let service = test_service("checker-post-reservation-unknown", Arc::clone(&launcher));
        service
            .replay_authority
            .allow_checker_authorization
            .store(false, Ordering::Release);

        let response = build_router(Arc::clone(&service))
            .oneshot(submission_request("```rust\nfn answer() {}\n```"))
            .await
            .expect("post-reservation failure response");
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        let body = response_json(response).await;
        assert_eq!(body["outcome"], "adjudication_unknown");
        assert_eq!(body["retryAuthorized"], false);
        assert_eq!(
            service
                .journal
                .lock()
                .expect("journal")
                .attempts
                .total_attempts(),
            1
        );
        assert_eq!(launcher.calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[tokio::test]
    async fn post_reservation_pre_intake_failure_is_unknown_and_never_authorizes_retry() {
        let launcher = TestLauncher::new([]);
        let service = test_service("pre-intake-post-reservation-unknown", Arc::clone(&launcher));
        service
            .replay_authority
            .allow_pre_intake_authorization
            .store(false, Ordering::Release);

        let response = build_router(Arc::clone(&service))
            .oneshot(submission_request("  \n"))
            .await
            .expect("post-reservation failure response");
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        let body = response_json(response).await;
        assert_eq!(body["outcome"], "adjudication_unknown");
        assert_eq!(body["retryAuthorized"], false);
        assert_eq!(
            service
                .journal
                .lock()
                .expect("journal")
                .attempts
                .total_attempts(),
            1
        );
        assert_eq!(launcher.calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[tokio::test]
    async fn grant_validation_failure_never_persists_a_pre_intake_attempt() {
        let launcher = TestLauncher::new([]);
        let service = test_service("pre-intake-auth-before-mutation", Arc::clone(&launcher));
        service
            .replay_authority
            .allow_pre_intake_validation
            .store(false, Ordering::Release);

        let response = build_router(Arc::clone(&service))
            .oneshot(submission_request("  \n"))
            .await
            .expect("authorization failure response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            service
                .journal
                .lock()
                .expect("journal")
                .attempts
                .total_attempts(),
            0
        );
        assert_eq!(launcher.calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn node_recomputes_terminal_retryable_and_resource_claim_outcomes_from_typed_facts() {
        use boole_native_shadow_protocol::{CheckerReason, CheckerVerdict};

        let base = LauncherAdjudicationFacts {
            cleanup_complete: true,
            timed_out: false,
            signaled: false,
            memory_events_max_delta: 0,
            pids_events_max_delta: 0,
            output_limit_exceeded: false,
            checker_verdict: Some(CheckerVerdict::Accepted),
            checker_reason: Some(CheckerReason::Accepted),
        };
        assert!(matches!(
            classify_launcher_facts(base),
            ValidatedLauncherOutcome::Terminal(ValidatedLauncherTerminal {
                verdict: ValidatedLauncherVerdict::Accepted,
                reason_code: "accepted"
            })
        ));

        let semantic_reject = LauncherAdjudicationFacts {
            checker_verdict: Some(CheckerVerdict::DeterministicReject),
            checker_reason: Some(CheckerReason::ForbiddenConstruct),
            ..base
        };
        assert!(matches!(
            classify_launcher_facts(semantic_reject),
            ValidatedLauncherOutcome::Terminal(ValidatedLauncherTerminal {
                verdict: ValidatedLauncherVerdict::DeterministicReject,
                reason_code: "checker_rejected"
            })
        ));

        let corroborated_process_limit = LauncherAdjudicationFacts {
            checker_verdict: Some(CheckerVerdict::RetryableUnavailable),
            checker_reason: Some(CheckerReason::ResourceProcessLimit),
            pids_events_max_delta: 1,
            ..base
        };
        assert!(matches!(
            classify_launcher_facts(corroborated_process_limit),
            ValidatedLauncherOutcome::Terminal(ValidatedLauncherTerminal {
                verdict: ValidatedLauncherVerdict::DeterministicReject,
                reason_code: "submission_resource_ceiling_breach"
            })
        ));

        let forged_memory_text = LauncherAdjudicationFacts {
            checker_verdict: Some(CheckerVerdict::RetryableUnavailable),
            checker_reason: Some(CheckerReason::ResourceMemoryLimit),
            ..base
        };
        assert!(matches!(
            classify_launcher_facts(forged_memory_text),
            ValidatedLauncherOutcome::Terminal(ValidatedLauncherTerminal {
                verdict: ValidatedLauncherVerdict::DeterministicReject,
                reason_code: "checker_reported_reason_unconfirmed"
            })
        ));

        let signaled_even_with_accept_text = LauncherAdjudicationFacts {
            signaled: true,
            ..base
        };
        assert!(matches!(
            classify_launcher_facts(signaled_even_with_accept_text),
            ValidatedLauncherOutcome::RetryableClean {
                reason: crate::native_shadow::NativeShadowRetryableReasonV3::ContainmentKilled
            }
        ));

        let incomplete_cleanup_even_with_accept = LauncherAdjudicationFacts {
            cleanup_complete: false,
            ..base
        };
        assert!(matches!(
            classify_launcher_facts(incomplete_cleanup_even_with_accept),
            ValidatedLauncherOutcome::Ambiguous {
                reason_code: "launcher_cleanup_incomplete"
            }
        ));
    }

    #[test]
    fn only_actual_attempt_exhaustion_is_returned_as_challenge_exhausted() {
        use crate::native_shadow::NativeShadowGrantAttemptErrorV1;

        assert_eq!(
            classify_attempt_reservation_error(
                &NativeShadowGrantAttemptErrorV1::CaseAlreadyReserved
            ),
            AttemptReservationDisposition::ChallengeExhausted
        );
        assert_eq!(
            classify_attempt_reservation_error(
                &NativeShadowGrantAttemptErrorV1::CheckerBudgetExceeded
            ),
            AttemptReservationDisposition::ChallengeExhausted
        );
        assert_eq!(
            classify_attempt_reservation_error(&NativeShadowGrantAttemptErrorV1::ScopeDrift),
            AttemptReservationDisposition::Poison
        );
        assert_eq!(
            classify_attempt_reservation_error(&NativeShadowGrantAttemptErrorV1::Durability(
                "disk full".to_string(),
            )),
            AttemptReservationDisposition::Poison
        );
    }

    #[test]
    fn execution_socket_accepts_only_the_qualified_root_launcher_instance() {
        let expected_instance = digest('9');
        assert!(validate_qualified_launcher_identity(
            4242,
            &expected_instance,
            LauncherPeerIdentity {
                pid: 4242,
                uid: 0,
                gid: 0,
            },
            4242,
            &expected_instance,
        )
        .is_ok());

        for rejected in [
            validate_qualified_launcher_identity(
                4242,
                &expected_instance,
                LauncherPeerIdentity {
                    pid: 4243,
                    uid: 0,
                    gid: 0,
                },
                4242,
                &expected_instance,
            ),
            validate_qualified_launcher_identity(
                4242,
                &expected_instance,
                LauncherPeerIdentity {
                    pid: 4242,
                    uid: 1000,
                    gid: 0,
                },
                4242,
                &expected_instance,
            ),
            validate_qualified_launcher_identity(
                4242,
                &expected_instance,
                LauncherPeerIdentity {
                    pid: 4242,
                    uid: 0,
                    gid: 0,
                },
                4243,
                &expected_instance,
            ),
            validate_qualified_launcher_identity(
                4242,
                &expected_instance,
                LauncherPeerIdentity {
                    pid: 4242,
                    uid: 0,
                    gid: 0,
                },
                4242,
                &digest('8'),
            ),
        ] {
            assert!(rejected.is_err());
        }
    }

    #[test]
    fn execution_transport_deadlines_are_the_frozen_policy_values() {
        let policy: Value =
            serde_json::from_slice(boole_native_shadow_protocol::TRACKED_EXECUTION_POLICY_BYTES)
                .expect("tracked execution policy JSON");
        assert_eq!(
            policy.pointer("/ipc/connectTimeoutMillis"),
            Some(&json!(LAUNCHER_CONNECT_TIMEOUT.as_millis() as u64))
        );
        assert_eq!(
            policy.pointer("/ipc/handshakeTimeoutMillis"),
            Some(&json!(LAUNCHER_HANDSHAKE_TIMEOUT.as_millis() as u64))
        );
        assert_eq!(
            policy.pointer("/ipc/executionResponseTimeoutMillis"),
            Some(&json!(LAUNCHER_EXECUTION_TIMEOUT.as_millis() as u64))
        );
        assert_eq!(LAUNCHER_EXECUTION_TIMEOUT, HTTP_TOTAL_RESPONSE_TIMEOUT);
    }

    #[test]
    fn installed_route_never_opens_with_unresolved_in_flight_rows() {
        assert!(ensure_no_unresolved_in_flight(&[]).is_ok());
        let stuck = NativeShadowFourTuple {
            family_version: "family-v1".to_string(),
            template_id: digest('1'),
            challenge_sha256: digest('2'),
            epoch: 3,
        };
        assert!(ensure_no_unresolved_in_flight(&[stuck]).is_err());
    }

    #[test]
    fn unresolved_in_flight_refuses_before_launcher_qualification() {
        let stuck = NativeShadowFourTuple {
            family_version: "family-v1".to_string(),
            template_id: digest('1'),
            challenge_sha256: digest('2'),
            epoch: 3,
        };
        let qualification_called = std::cell::Cell::new(false);

        let result = qualify_after_recovery_gate(&[stuck], || {
            qualification_called.set(true);
            Ok(())
        });

        assert!(result.is_err());
        assert!(
            !qualification_called.get(),
            "durable unresolved work must fail closed before expensive launcher qualification"
        );
    }
}
