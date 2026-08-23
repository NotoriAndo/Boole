use std::io::{self, Read, Write};
use std::num::NonZeroU32;

use boole_native_shadow_protocol::{
    read_qualification_hello, write_qualification_ready, QualificationReady,
    QualificationReadyFields, VerifiedAuthorityBundle, WireError,
};
use thiserror::Error;

use crate::toolchain_compatibility::VerifiedStartupToolchainCompatibility;

pub(crate) mod listener;
pub use listener::{serve_one_fixed_unix_qualification, FixedQualificationListenerError};

#[cfg(target_os = "linux")]
mod unix;

mod private {
    pub trait Sealed {}
}

/// Kernel-authenticated credentials for the connected node process.
///
/// Construction stays inside this crate so a caller cannot substitute claimed
/// numeric identities for credentials observed by the Unix socket adapter.
///
/// ```compile_fail
/// use boole_native_shadow_launcher::qualification::NodePeerCredentials;
/// let _forged = NodePeerCredentials { pid: 1, uid: 2, gid: 2 };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodePeerCredentials {
    pid: u32,
    uid: u32,
    gid: u32,
}

/// Opaque proof that launcher startup authority, identities and recovery were
/// verified before any readiness response can be served.
///
/// This type deliberately exposes no constructor. The crate's fixed readiness
/// assembler creates it only after root identity, installed authority, fixed
/// NSS identities, fresh launcher ID, zero-leaf recovery and the four trusted
/// toolchain compatibility probes all pass.
///
/// ```compile_fail
/// use std::num::NonZeroU32;
/// use boole_native_shadow_launcher::qualification::VerifiedQualificationStartup;
/// let _forged = VerifiedQualificationStartup {
///     authority: panic!("no verified authority"),
///     launcher_pid: NonZeroU32::new(1).unwrap(),
///     launcher_instance_id: [0; 32],
///     node_uid: 2,
///     node_gid: 2,
///     checker_uid: 3,
///     checker_gid: 3,
/// };
/// ```
///
/// ```compile_fail
/// use boole_native_shadow_launcher::qualification::VerifiedQualificationStartup;
/// fn require_send<T: Send>() {}
/// require_send::<VerifiedQualificationStartup>();
/// ```
///
/// ```compile_fail
/// use boole_native_shadow_launcher::qualification::VerifiedQualificationStartup;
/// fn require_sync<T: Sync>() {}
/// require_sync::<VerifiedQualificationStartup>();
/// ```
pub struct VerifiedQualificationStartup {
    guard: QualificationStartupGuard,
    authority: VerifiedAuthorityBundle,
    launcher_pid: NonZeroU32,
    launcher_instance_id: [u8; 32],
    node_uid: u32,
    node_gid: u32,
    checker_uid: u32,
    checker_gid: u32,
}

enum QualificationStartupGuard {
    Verified(Box<VerifiedStartupToolchainCompatibility>),
    #[cfg(test)]
    Test,
}

impl std::fmt::Debug for VerifiedQualificationStartup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedQualificationStartup")
            .field("guard", &"verified startup chain")
            .field("authority", &self.authority)
            .field("launcher_pid", &self.launcher_pid)
            .field("launcher_instance_id", &self.launcher_instance_id)
            .field("node_uid", &self.node_uid)
            .field("node_gid", &self.node_gid)
            .field("checker_uid", &self.checker_uid)
            .field("checker_gid", &self.checker_gid)
            .finish()
    }
}

impl VerifiedQualificationStartup {
    pub(crate) fn from_verified_toolchain(
        compatibility: VerifiedStartupToolchainCompatibility,
    ) -> Result<Self, crate::readiness::QualificationStartupError> {
        let (authority, identities, launcher_instance_id) = {
            let recovery = compatibility.recovery();
            let instance = recovery.manager().instance();
            let prerequisites = instance.lifetime_lock().prerequisites();
            (
                prerequisites.authority().clone(),
                prerequisites.identities(),
                instance.instance_id(),
            )
        };
        let launcher_pid = crate::readiness::require_nonzero_launcher_pid(std::process::id())?;
        Ok(Self {
            guard: QualificationStartupGuard::Verified(Box::new(compatibility)),
            authority,
            launcher_pid,
            launcher_instance_id,
            node_uid: identities.node_uid(),
            node_gid: identities.node_gid(),
            checker_uid: identities.checker_uid(),
            checker_gid: identities.checker_gid(),
        })
    }

    pub(crate) fn verified_toolchain(&self) -> &VerifiedStartupToolchainCompatibility {
        match &self.guard {
            QualificationStartupGuard::Verified(compatibility) => compatibility.as_ref(),
            #[cfg(test)]
            QualificationStartupGuard::Test => {
                panic!("test-only qualification startup has no production proof chain")
            }
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn runtime_directory(&self) -> &std::fs::File {
        self.verified_toolchain()
            .recovery()
            .manager()
            .instance()
            .lifetime_lock()
            .runtime_directory()
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn node_gid(&self) -> u32 {
        self.node_gid
    }
}

/// Session operations required by the behavioral handshake core.
///
/// The trait is sealed: the future production Unix adapter lives in this
/// crate, while tests use an in-crate deterministic session.
pub trait QualificationSession: Read + Write + private::Sealed {
    fn peer_credentials(&mut self) -> io::Result<NodePeerCredentials>;
    fn shutdown_write(&mut self) -> io::Result<()>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum QualificationServerError {
    #[error("node peer credentials are unavailable: {0}")]
    PeerCredentialsUnavailable(String),
    #[error("qualification peer does not match the fixed boole-node identity")]
    UntrustedNodePeer,
    #[error(transparent)]
    Wire(#[from] WireError),
    #[error("node closed before sending qualification hello")]
    PrematureEof,
    #[error("qualification binding mismatch for {field}")]
    BindingMismatch { field: &'static str },
    #[error("failed to flush qualification ready: {0}")]
    FlushReady(String),
    #[error("node sent data after qualification ready")]
    UnexpectedPostReadyFrame,
    #[error("failed to shut down launcher write half: {0}")]
    ShutdownWrite(String),
}

/// Serve exactly one disabled, request-free qualification exchange.
///
/// The owned session is never returned, so a failed connection cannot be
/// reused. Success yields no route, execution or activation capability.
pub fn serve_request_free_qualification<S>(
    mut session: S,
    startup: &VerifiedQualificationStartup,
) -> Result<(), QualificationServerError>
where
    S: QualificationSession,
{
    let peer = session
        .peer_credentials()
        .map_err(|error| QualificationServerError::PeerCredentialsUnavailable(error.to_string()))?;
    if peer.pid == 0 || peer.uid != startup.node_uid || peer.gid != startup.node_gid {
        return Err(QualificationServerError::UntrustedNodePeer);
    }

    let hello =
        read_qualification_hello(&mut session)?.ok_or(QualificationServerError::PrematureEof)?;
    require_binding(
        "executionPolicyDigestHex",
        startup.authority.execution_policy_digest(),
        hello.execution_policy_digest_hex(),
    )?;
    require_binding(
        "toolchainIdentityDigestHex",
        startup.authority.toolchain_identity_digest(),
        hello.toolchain_identity_digest_hex(),
    )?;
    require_binding(
        "registryDigestHex",
        startup.authority.registry_digest(),
        hello.registry_digest_hex(),
    )?;

    let ready = QualificationReady::try_new(QualificationReadyFields {
        nonce_hex: hello.nonce_hex().to_string(),
        execution_policy_digest_hex: startup.authority.execution_policy_digest().to_string(),
        toolchain_identity_digest_hex: startup.authority.toolchain_identity_digest().to_string(),
        registry_digest_hex: startup.authority.registry_digest().to_string(),
        launcher_pid: startup.launcher_pid.get(),
        launcher_uid: 0,
        launcher_gid: 0,
        node_uid: startup.node_uid,
        node_gid: startup.node_gid,
        checker_uid: startup.checker_uid,
        checker_gid: startup.checker_gid,
        startup_recovery_complete: true,
        active_execution_leaves: 0,
        unexpected_direct_cgroup_children: 0,
        manager_subgroup_verified: true,
        launcher_instance_id_hex: hex::encode(startup.launcher_instance_id),
        activation_allowed: false,
        ready: true,
    })?;
    write_qualification_ready(&mut session, &ready)?;
    session
        .flush()
        .map_err(|error| QualificationServerError::FlushReady(error.to_string()))?;

    if read_qualification_hello(&mut session)?.is_some() {
        return Err(QualificationServerError::UnexpectedPostReadyFrame);
    }
    session
        .shutdown_write()
        .map_err(|error| QualificationServerError::ShutdownWrite(error.to_string()))?;
    Ok(())
}

fn require_binding(
    field: &'static str,
    expected: &str,
    actual: &str,
) -> Result<(), QualificationServerError> {
    if expected == actual {
        Ok(())
    } else {
        Err(QualificationServerError::BindingMismatch { field })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io::{self, Read, Write};
    use std::num::NonZeroU32;
    use std::rc::Rc;

    use boole_native_shadow_protocol::{
        decode_complete_qualification_ready_frame, encode_qualification_hello_frame,
        verify_authority_bundle, QualificationHello, TRACKED_EXECUTION_POLICY_BYTES,
        TRACKED_REGISTRY_BYTES, TRACKED_TOOLCHAIN_IDENTITY_BYTES,
    };

    use super::{
        private, serve_request_free_qualification, NodePeerCredentials, QualificationServerError,
        QualificationSession, QualificationStartupGuard, VerifiedQualificationStartup,
    };

    const NODE_UID: u32 = 20_001;
    const NODE_GID: u32 = 20_001;
    const CHECKER_UID: u32 = 20_002;
    const CHECKER_GID: u32 = 20_002;
    const LAUNCHER_PID: u32 = 4_242;
    const NODE_PID: u32 = 8_484;
    const INSTANCE_ID: [u8; 32] = [0x5a; 32];
    const NONCE: &str = "abababababababababababababababababababababababababababababababab";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Event {
        PeerCredentials,
        ReadHello,
        WriteReady,
        FlushReady,
        ReadPostReady,
        ShutdownWrite,
    }

    #[derive(Debug, Default)]
    struct Observation {
        events: Vec<Event>,
        output: Vec<u8>,
        read_started: bool,
        post_ready_read_started: bool,
        ready_write_started: bool,
    }

    #[derive(Debug, Clone, Copy)]
    enum PeerOutcome {
        Credentials(NodePeerCredentials),
        Unavailable,
    }

    struct MockSession {
        input: Vec<u8>,
        offset: usize,
        observation: Rc<RefCell<Observation>>,
        peer: PeerOutcome,
        fail_flush: bool,
        fail_shutdown: bool,
    }

    impl MockSession {
        fn new(input: Vec<u8>) -> (Self, Rc<RefCell<Observation>>) {
            let observation = Rc::new(RefCell::new(Observation::default()));
            (
                Self {
                    input,
                    offset: 0,
                    observation: Rc::clone(&observation),
                    peer: PeerOutcome::Credentials(trusted_peer()),
                    fail_flush: false,
                    fail_shutdown: false,
                },
                observation,
            )
        }
    }

    impl private::Sealed for MockSession {}

    impl QualificationSession for MockSession {
        fn peer_credentials(&mut self) -> io::Result<NodePeerCredentials> {
            self.observation
                .borrow_mut()
                .events
                .push(Event::PeerCredentials);
            match self.peer {
                PeerOutcome::Credentials(peer) => Ok(peer),
                PeerOutcome::Unavailable => Err(io::Error::other("peer unavailable")),
            }
        }

        fn shutdown_write(&mut self) -> io::Result<()> {
            self.observation
                .borrow_mut()
                .events
                .push(Event::ShutdownWrite);
            if self.fail_shutdown {
                Err(io::Error::other("shutdown failed"))
            } else {
                Ok(())
            }
        }
    }

    impl Read for MockSession {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            {
                let mut observation = self.observation.borrow_mut();
                if !observation.read_started {
                    observation.read_started = true;
                    observation.events.push(Event::ReadHello);
                } else if observation.ready_write_started && !observation.post_ready_read_started {
                    observation.post_ready_read_started = true;
                    observation.events.push(Event::ReadPostReady);
                }
            }

            let remaining = &self.input[self.offset..];
            let count = remaining.len().min(buffer.len());
            buffer[..count].copy_from_slice(&remaining[..count]);
            self.offset += count;
            Ok(count)
        }
    }

    impl Write for MockSession {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            let mut observation = self.observation.borrow_mut();
            if !observation.ready_write_started {
                observation.ready_write_started = true;
                observation.events.push(Event::WriteReady);
            }
            observation.output.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.observation.borrow_mut().events.push(Event::FlushReady);
            if self.fail_flush {
                Err(io::Error::other("flush failed"))
            } else {
                Ok(())
            }
        }
    }

    fn authority() -> boole_native_shadow_protocol::VerifiedAuthorityBundle {
        verify_authority_bundle(
            TRACKED_REGISTRY_BYTES,
            TRACKED_EXECUTION_POLICY_BYTES,
            TRACKED_TOOLCHAIN_IDENTITY_BYTES,
        )
        .expect("tracked authority verifies")
    }

    fn startup() -> VerifiedQualificationStartup {
        VerifiedQualificationStartup {
            guard: QualificationStartupGuard::Test,
            authority: authority(),
            launcher_pid: NonZeroU32::new(LAUNCHER_PID).expect("non-zero test PID"),
            launcher_instance_id: INSTANCE_ID,
            node_uid: NODE_UID,
            node_gid: NODE_GID,
            checker_uid: CHECKER_UID,
            checker_gid: CHECKER_GID,
        }
    }

    fn trusted_peer() -> NodePeerCredentials {
        NodePeerCredentials {
            pid: NODE_PID,
            uid: NODE_UID,
            gid: NODE_GID,
        }
    }

    fn hello_with(
        execution_policy_digest: &str,
        toolchain_identity_digest: &str,
        registry_digest: &str,
    ) -> Vec<u8> {
        encode_qualification_hello_frame(
            &QualificationHello::try_new(
                NONCE.to_string(),
                execution_policy_digest.to_string(),
                toolchain_identity_digest.to_string(),
                registry_digest.to_string(),
            )
            .expect("test hello validates"),
        )
        .expect("test hello encodes")
    }

    fn valid_hello(startup: &VerifiedQualificationStartup) -> Vec<u8> {
        hello_with(
            startup.authority.execution_policy_digest(),
            startup.authority.toolchain_identity_digest(),
            startup.authority.registry_digest(),
        )
    }

    #[test]
    fn happy_path_binds_ready_and_obeys_exact_request_free_order() {
        let startup = startup();
        let (session, observation) = MockSession::new(valid_hello(&startup));

        serve_request_free_qualification(session, &startup).expect("qualification succeeds");

        let observation = observation.borrow();
        assert_eq!(
            observation.events,
            [
                Event::PeerCredentials,
                Event::ReadHello,
                Event::WriteReady,
                Event::FlushReady,
                Event::ReadPostReady,
                Event::ShutdownWrite,
            ]
        );
        let ready = decode_complete_qualification_ready_frame(&observation.output)
            .expect("launcher emitted one strict ready frame");
        assert_eq!(ready.nonce_hex(), NONCE);
        assert_eq!(
            ready.execution_policy_digest_hex(),
            startup.authority.execution_policy_digest()
        );
        assert_eq!(
            ready.toolchain_identity_digest_hex(),
            startup.authority.toolchain_identity_digest()
        );
        assert_eq!(
            ready.registry_digest_hex(),
            startup.authority.registry_digest()
        );
        assert_eq!(ready.launcher_pid(), LAUNCHER_PID);
        assert_eq!((ready.launcher_uid(), ready.launcher_gid()), (0, 0));
        assert_eq!((ready.node_uid(), ready.node_gid()), (NODE_UID, NODE_GID));
        assert_eq!(
            (ready.checker_uid(), ready.checker_gid()),
            (CHECKER_UID, CHECKER_GID)
        );
        assert!(ready.startup_recovery_complete());
        assert_eq!(ready.active_execution_leaves(), 0);
        assert_eq!(ready.unexpected_direct_cgroup_children(), 0);
        assert!(ready.manager_subgroup_verified());
        assert_eq!(ready.launcher_instance_id_hex(), hex::encode(INSTANCE_ID));
        assert!(!ready.activation_allowed());
        assert!(ready.ready());
    }

    #[test]
    fn peer_failures_happen_before_any_frame_io() {
        let startup = startup();
        let cases = [
            PeerOutcome::Unavailable,
            PeerOutcome::Credentials(NodePeerCredentials {
                pid: 0,
                ..trusted_peer()
            }),
            PeerOutcome::Credentials(NodePeerCredentials {
                uid: NODE_UID + 1,
                ..trusted_peer()
            }),
            PeerOutcome::Credentials(NodePeerCredentials {
                gid: NODE_GID + 1,
                ..trusted_peer()
            }),
        ];

        for peer in cases {
            let (mut session, observation) = MockSession::new(valid_hello(&startup));
            session.peer = peer;
            let error = serve_request_free_qualification(session, &startup)
                .expect_err("untrusted peer must fail");
            assert!(matches!(
                error,
                QualificationServerError::PeerCredentialsUnavailable(_)
                    | QualificationServerError::UntrustedNodePeer
            ));
            let observation = observation.borrow();
            assert_eq!(observation.events, [Event::PeerCredentials]);
            assert!(observation.output.is_empty());
        }
    }

    #[test]
    fn every_authority_mismatch_fails_before_ready() {
        let startup = startup();
        let wrong = "11".repeat(32);
        let cases = [
            (
                "executionPolicyDigestHex",
                hello_with(
                    &wrong,
                    startup.authority.toolchain_identity_digest(),
                    startup.authority.registry_digest(),
                ),
            ),
            (
                "toolchainIdentityDigestHex",
                hello_with(
                    startup.authority.execution_policy_digest(),
                    &wrong,
                    startup.authority.registry_digest(),
                ),
            ),
            (
                "registryDigestHex",
                hello_with(
                    startup.authority.execution_policy_digest(),
                    startup.authority.toolchain_identity_digest(),
                    &wrong,
                ),
            ),
        ];

        for (field, input) in cases {
            let (session, observation) = MockSession::new(input);
            assert_eq!(
                serve_request_free_qualification(session, &startup),
                Err(QualificationServerError::BindingMismatch { field })
            );
            let observation = observation.borrow();
            assert_eq!(
                observation.events,
                [Event::PeerCredentials, Event::ReadHello]
            );
            assert!(observation.output.is_empty());
        }
    }

    #[test]
    fn eof_before_hello_emits_no_ready() {
        let startup = startup();
        let (session, observation) = MockSession::new(Vec::new());

        assert_eq!(
            serve_request_free_qualification(session, &startup),
            Err(QualificationServerError::PrematureEof)
        );
        let observation = observation.borrow();
        assert_eq!(
            observation.events,
            [Event::PeerCredentials, Event::ReadHello]
        );
        assert!(observation.output.is_empty());
    }

    #[test]
    fn any_second_frame_is_rejected_without_shutdown_success() {
        let startup = startup();
        let hello = valid_hello(&startup);
        let mut two_frames = hello.clone();
        two_frames.extend_from_slice(&hello);
        let (session, observation) = MockSession::new(two_frames);

        assert_eq!(
            serve_request_free_qualification(session, &startup),
            Err(QualificationServerError::UnexpectedPostReadyFrame)
        );
        let observation = observation.borrow();
        assert_eq!(
            observation.events,
            [
                Event::PeerCredentials,
                Event::ReadHello,
                Event::WriteReady,
                Event::FlushReady,
                Event::ReadPostReady,
            ]
        );
        assert!(!observation.output.is_empty());
    }

    #[test]
    fn flush_and_shutdown_failures_are_not_reported_as_success() {
        let startup = startup();

        let (mut flush_session, flush_observation) = MockSession::new(valid_hello(&startup));
        flush_session.fail_flush = true;
        assert!(matches!(
            serve_request_free_qualification(flush_session, &startup),
            Err(QualificationServerError::FlushReady(_))
        ));
        assert_eq!(
            flush_observation.borrow().events,
            [
                Event::PeerCredentials,
                Event::ReadHello,
                Event::WriteReady,
                Event::FlushReady,
            ]
        );

        let (mut shutdown_session, shutdown_observation) = MockSession::new(valid_hello(&startup));
        shutdown_session.fail_shutdown = true;
        assert!(matches!(
            serve_request_free_qualification(shutdown_session, &startup),
            Err(QualificationServerError::ShutdownWrite(_))
        ));
        assert_eq!(
            shutdown_observation.borrow().events,
            [
                Event::PeerCredentials,
                Event::ReadHello,
                Event::WriteReady,
                Event::FlushReady,
                Event::ReadPostReady,
                Event::ShutdownWrite,
            ]
        );
    }
}
