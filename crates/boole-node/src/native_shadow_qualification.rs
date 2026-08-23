use std::io::{self, Read, Write};

use boole_native_shadow_protocol::{
    read_qualification_ready, write_qualification_hello, QualificationHello, QualificationReady,
    VerifiedAuthorityBundle, WireError,
};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeShadowPeerCredentials {
    pid: u32,
    uid: u32,
    gid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeShadowExpectedIdentities {
    node_uid: u32,
    node_gid: u32,
    checker_uid: u32,
    checker_gid: u32,
}

impl NativeShadowExpectedIdentities {
    fn validate(self) -> Result<Self, NativeShadowQualificationError> {
        if self.node_uid == 0
            || self.node_gid == 0
            || self.checker_uid == 0
            || self.checker_gid == 0
            || self.node_uid == self.checker_uid
            || self.node_gid == self.checker_gid
        {
            return Err(NativeShadowQualificationError::InvalidExpectedIdentities);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QualificationNonce([u8; 32]);

impl QualificationNonce {
    // Phase 3B.2b-1 injects fixed bytes only into the mock-owned client. The
    // real getrandom(2)-only source belongs to the later Linux socket slice.
    fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    fn encode_hex(self) -> String {
        hex::encode(self.0)
    }
}

trait NativeShadowQualificationSession: Read + Write {
    fn peer_credentials(&mut self) -> io::Result<NativeShadowPeerCredentials>;
    fn shutdown_write(&mut self) -> io::Result<()>;
}

/// Successful, in-memory readiness from a disabled qualification exchange.
///
/// This type deliberately has no lifecycle, journal, route, or execution API.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeShadowQualificationReadiness {
    launcher_pid: u32,
    launcher_instance_id_hex: String,
    registry_digest_hex: String,
    execution_policy_digest_hex: String,
    toolchain_identity_digest_hex: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
enum NativeShadowQualificationError {
    #[error("launcher peer credentials are unavailable: {0}")]
    PeerCredentialsUnavailable(String),
    #[error("launcher peer must be root with a non-zero PID")]
    UntrustedPeer,
    #[error("expected node/checker identities must be non-root and mutually distinct")]
    InvalidExpectedIdentities,
    #[error(transparent)]
    Wire(#[from] WireError),
    #[error("launcher closed before qualification-ready")]
    PrematureEof,
    #[error("qualification binding mismatch for {field}: expected {expected}, got {actual}")]
    BindingMismatch {
        field: &'static str,
        expected: String,
        actual: String,
    },
    #[error("failed to shut down the node write half: {0}")]
    ShutdownWrite(String),
    #[error("launcher sent a second qualification-ready frame after node shutdown-write")]
    UnexpectedSecondReady,
}

fn require_binding<T>(
    field: &'static str,
    expected: T,
    actual: T,
) -> Result<(), NativeShadowQualificationError>
where
    T: PartialEq + ToString,
{
    if expected != actual {
        return Err(NativeShadowQualificationError::BindingMismatch {
            field,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}

fn validate_ready_bindings(
    ready: &QualificationReady,
    peer: NativeShadowPeerCredentials,
    expected: NativeShadowExpectedIdentities,
    nonce_hex: &str,
    authority: &VerifiedAuthorityBundle,
) -> Result<(), NativeShadowQualificationError> {
    require_binding("nonceHex", nonce_hex, ready.nonce_hex())?;
    require_binding(
        "registryDigestHex",
        authority.registry_digest(),
        ready.registry_digest_hex(),
    )?;
    require_binding(
        "executionPolicyDigestHex",
        authority.execution_policy_digest(),
        ready.execution_policy_digest_hex(),
    )?;
    require_binding(
        "toolchainIdentityDigestHex",
        authority.toolchain_identity_digest(),
        ready.toolchain_identity_digest_hex(),
    )?;
    require_binding("launcherPid", peer.pid, ready.launcher_pid())?;
    require_binding("launcherUid", peer.uid, ready.launcher_uid())?;
    require_binding("launcherGid", peer.gid, ready.launcher_gid())?;
    require_binding("nodeUid", expected.node_uid, ready.node_uid())?;
    require_binding("nodeGid", expected.node_gid, ready.node_gid())?;
    require_binding("checkerUid", expected.checker_uid, ready.checker_uid())?;
    require_binding("checkerGid", expected.checker_gid, ready.checker_gid())?;
    Ok(())
}

fn qualify_native_shadow_launcher<S>(
    mut session: S,
    authority: &VerifiedAuthorityBundle,
    nonce: QualificationNonce,
    expected: NativeShadowExpectedIdentities,
) -> Result<NativeShadowQualificationReadiness, NativeShadowQualificationError>
where
    S: NativeShadowQualificationSession,
{
    let peer = session.peer_credentials().map_err(|error| {
        NativeShadowQualificationError::PeerCredentialsUnavailable(error.to_string())
    })?;
    if peer.pid == 0 || peer.uid != 0 || peer.gid != 0 {
        return Err(NativeShadowQualificationError::UntrustedPeer);
    }
    let expected = expected.validate()?;
    let nonce_hex = nonce.encode_hex();
    let hello = QualificationHello::try_new(
        nonce_hex.clone(),
        authority.execution_policy_digest().to_string(),
        authority.toolchain_identity_digest().to_string(),
        authority.registry_digest().to_string(),
    )?;

    write_qualification_hello(&mut session, &hello)?;
    let ready = read_qualification_ready(&mut session)?
        .ok_or(NativeShadowQualificationError::PrematureEof)?;
    validate_ready_bindings(&ready, peer, expected, &nonce_hex, authority)?;

    session
        .shutdown_write()
        .map_err(|error| NativeShadowQualificationError::ShutdownWrite(error.to_string()))?;
    if read_qualification_ready(&mut session)?.is_some() {
        return Err(NativeShadowQualificationError::UnexpectedSecondReady);
    }

    Ok(NativeShadowQualificationReadiness {
        launcher_pid: ready.launcher_pid(),
        launcher_instance_id_hex: ready.launcher_instance_id_hex().to_string(),
        registry_digest_hex: ready.registry_digest_hex().to_string(),
        execution_policy_digest_hex: ready.execution_policy_digest_hex().to_string(),
        toolchain_identity_digest_hex: ready.toolchain_identity_digest_hex().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io::{self, Cursor, Read, Write};
    use std::rc::Rc;

    use boole_native_shadow_protocol::{
        decode_complete_qualification_hello_frame, encode_qualification_ready_frame,
        verify_authority_bundle, QualificationReady, QualificationReadyFields,
        MAX_RESPONSE_FRAME_BYTES, TRACKED_EXECUTION_POLICY_BYTES, TRACKED_REGISTRY_BYTES,
        TRACKED_TOOLCHAIN_IDENTITY_BYTES,
    };
    use serde_json::{json, Value};

    use super::{
        qualify_native_shadow_launcher, NativeShadowExpectedIdentities,
        NativeShadowPeerCredentials, NativeShadowQualificationError,
        NativeShadowQualificationSession, QualificationNonce,
    };

    const NONCE: [u8; 32] = [0x42; 32];
    const LAUNCHER_INSTANCE_ID: &str =
        "abababababababababababababababababababababababababababababababab";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Event {
        PeerCredentials,
        WriteHello,
        ReadReady,
        ShutdownWrite,
        ReadEof,
    }

    struct MockSession {
        peer: Option<NativeShadowPeerCredentials>,
        response: Cursor<Vec<u8>>,
        observation: Rc<RefCell<MockObservation>>,
        ready_read_recorded: bool,
        shutdown: bool,
        require_shutdown_for_eof: bool,
        post_shutdown_eof_fails: bool,
        shutdown_fails: bool,
    }

    #[derive(Default)]
    struct MockObservation {
        written: Vec<u8>,
        events: Vec<Event>,
        shutdown: bool,
    }

    impl MockSession {
        fn new(peer: NativeShadowPeerCredentials, response: Vec<u8>) -> Self {
            Self {
                peer: Some(peer),
                response: Cursor::new(response),
                observation: Rc::new(RefCell::new(MockObservation::default())),
                ready_read_recorded: false,
                shutdown: false,
                require_shutdown_for_eof: true,
                post_shutdown_eof_fails: false,
                shutdown_fails: false,
            }
        }

        fn allow_early_eof(mut self) -> Self {
            self.require_shutdown_for_eof = false;
            self
        }

        fn observation(&self) -> Rc<RefCell<MockObservation>> {
            Rc::clone(&self.observation)
        }
    }

    impl Read for MockSession {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.response.position() < self.response.get_ref().len() as u64 {
                if !self.ready_read_recorded {
                    self.observation.borrow_mut().events.push(Event::ReadReady);
                    self.ready_read_recorded = true;
                }
                return self.response.read(buffer);
            }
            self.observation.borrow_mut().events.push(Event::ReadEof);
            if self.post_shutdown_eof_fails && self.shutdown {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "mock launcher never completed its shutdown-write",
                ));
            }
            if self.require_shutdown_for_eof && !self.shutdown {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "launcher EOF is not available before node shutdown-write",
                ));
            }
            Ok(0)
        }
    }

    impl Write for MockSession {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            let mut observation = self.observation.borrow_mut();
            observation.events.push(Event::WriteHello);
            observation.written.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl NativeShadowQualificationSession for MockSession {
        fn peer_credentials(&mut self) -> io::Result<NativeShadowPeerCredentials> {
            self.observation
                .borrow_mut()
                .events
                .push(Event::PeerCredentials);
            self.peer
                .ok_or_else(|| io::Error::other("SO_PEERCRED unavailable"))
        }

        fn shutdown_write(&mut self) -> io::Result<()> {
            self.observation
                .borrow_mut()
                .events
                .push(Event::ShutdownWrite);
            if self.shutdown_fails {
                return Err(io::Error::other("mock shutdown failure"));
            }
            self.shutdown = true;
            self.observation.borrow_mut().shutdown = true;
            Ok(())
        }
    }

    fn authority() -> boole_native_shadow_protocol::VerifiedAuthorityBundle {
        verify_authority_bundle(
            TRACKED_REGISTRY_BYTES,
            TRACKED_EXECUTION_POLICY_BYTES,
            TRACKED_TOOLCHAIN_IDENTITY_BYTES,
        )
        .expect("tracked authorities must verify")
    }

    fn peer() -> NativeShadowPeerCredentials {
        NativeShadowPeerCredentials {
            pid: 4242,
            uid: 0,
            gid: 0,
        }
    }

    fn expected_identities() -> NativeShadowExpectedIdentities {
        NativeShadowExpectedIdentities {
            node_uid: 1000,
            node_gid: 1000,
            checker_uid: 1001,
            checker_gid: 1001,
        }
    }

    fn ready_fields() -> QualificationReadyFields {
        let authority = authority();
        QualificationReadyFields {
            nonce_hex: hex::encode(NONCE),
            execution_policy_digest_hex: authority.execution_policy_digest().to_string(),
            toolchain_identity_digest_hex: authority.toolchain_identity_digest().to_string(),
            registry_digest_hex: authority.registry_digest().to_string(),
            launcher_pid: peer().pid,
            launcher_uid: peer().uid,
            launcher_gid: peer().gid,
            node_uid: expected_identities().node_uid,
            node_gid: expected_identities().node_gid,
            checker_uid: expected_identities().checker_uid,
            checker_gid: expected_identities().checker_gid,
            startup_recovery_complete: true,
            active_execution_leaves: 0,
            unexpected_direct_cgroup_children: 0,
            manager_subgroup_verified: true,
            launcher_instance_id_hex: LAUNCHER_INSTANCE_ID.to_string(),
            activation_allowed: false,
            ready: true,
        }
    }

    fn ready_frame(fields: QualificationReadyFields) -> Vec<u8> {
        encode_qualification_ready_frame(
            &QualificationReady::try_new(fields).expect("valid ready fields"),
        )
        .expect("ready frame encodes")
    }

    fn valid_ready_frame() -> Vec<u8> {
        ready_frame(ready_fields())
    }

    fn raw_mutated_ready_frame(field: &str, replacement: Value) -> Vec<u8> {
        let frame = valid_ready_frame();
        let mut value: Value = serde_json::from_slice(&frame[4..]).expect("valid ready JSON");
        value[field] = replacement;
        let payload = serde_json::to_vec(&value).expect("mutated JSON encodes");
        let mut mutated = Vec::with_capacity(4 + payload.len());
        mutated.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        mutated.extend_from_slice(&payload);
        mutated
    }

    fn qualify(
        session: MockSession,
    ) -> Result<super::NativeShadowQualificationReadiness, NativeShadowQualificationError> {
        qualify_native_shadow_launcher(
            session,
            &authority(),
            QualificationNonce::from_bytes(NONCE),
            expected_identities(),
        )
    }

    #[test]
    fn happy_path_binds_hello_then_ready_then_shutdown_then_eof() {
        let session = MockSession::new(peer(), valid_ready_frame());
        let observation = session.observation();

        let readiness = qualify(session).expect("qualification must succeed");

        assert_eq!(readiness.launcher_pid, peer().pid);
        assert_eq!(readiness.launcher_instance_id_hex, LAUNCHER_INSTANCE_ID);
        let observation = observation.borrow();
        assert_eq!(
            observation.events,
            [
                Event::PeerCredentials,
                Event::WriteHello,
                Event::ReadReady,
                Event::ShutdownWrite,
                Event::ReadEof,
            ]
        );
        let hello = decode_complete_qualification_hello_frame(&observation.written)
            .expect("node must emit one strict hello frame");
        let authority = authority();
        assert_eq!(hello.nonce_hex(), hex::encode(NONCE));
        assert_eq!(hello.registry_digest_hex(), authority.registry_digest());
        assert_eq!(
            hello.execution_policy_digest_hex(),
            authority.execution_policy_digest()
        );
        assert_eq!(
            hello.toolchain_identity_digest_hex(),
            authority.toolchain_identity_digest()
        );
    }

    #[test]
    fn rejects_untrusted_or_unavailable_peer_before_writing_hello() {
        for bad_peer in [
            NativeShadowPeerCredentials {
                pid: 0,
                uid: 0,
                gid: 0,
            },
            NativeShadowPeerCredentials {
                pid: 42,
                uid: 1,
                gid: 0,
            },
            NativeShadowPeerCredentials {
                pid: 42,
                uid: 0,
                gid: 1,
            },
        ] {
            let session = MockSession::new(bad_peer, valid_ready_frame());
            let observation = session.observation();
            assert_eq!(
                qualify(session),
                Err(NativeShadowQualificationError::UntrustedPeer)
            );
            assert!(observation.borrow().written.is_empty());
        }

        let mut unavailable = MockSession::new(peer(), valid_ready_frame());
        unavailable.peer = None;
        let observation = unavailable.observation();
        assert!(matches!(
            qualify(unavailable),
            Err(NativeShadowQualificationError::PeerCredentialsUnavailable(
                _
            ))
        ));
        assert!(observation.borrow().written.is_empty());
    }

    #[test]
    fn rejects_invalid_expected_identities_before_writing_hello() {
        let authority = authority();
        for expected in [
            NativeShadowExpectedIdentities {
                node_uid: 0,
                ..expected_identities()
            },
            NativeShadowExpectedIdentities {
                checker_gid: 0,
                ..expected_identities()
            },
            NativeShadowExpectedIdentities {
                checker_uid: expected_identities().node_uid,
                ..expected_identities()
            },
            NativeShadowExpectedIdentities {
                checker_gid: expected_identities().node_gid,
                ..expected_identities()
            },
        ] {
            let session = MockSession::new(peer(), valid_ready_frame());
            let observation = session.observation();
            assert_eq!(
                qualify_native_shadow_launcher(
                    session,
                    &authority,
                    QualificationNonce::from_bytes(NONCE),
                    expected,
                ),
                Err(NativeShadowQualificationError::InvalidExpectedIdentities)
            );
            assert!(observation.borrow().written.is_empty());
        }
    }

    #[test]
    fn rejects_nonce_and_each_authority_digest_mismatch() {
        let mut variants = Vec::new();
        let mut nonce = ready_fields();
        nonce.nonce_hex = "11".repeat(32);
        variants.push(("nonceHex", nonce));
        let mut registry = ready_fields();
        registry.registry_digest_hex = "22".repeat(32);
        variants.push(("registryDigestHex", registry));
        let mut policy = ready_fields();
        policy.execution_policy_digest_hex = "33".repeat(32);
        variants.push(("executionPolicyDigestHex", policy));
        let mut toolchain = ready_fields();
        toolchain.toolchain_identity_digest_hex = "44".repeat(32);
        variants.push(("toolchainIdentityDigestHex", toolchain));

        for (expected_field, fields) in variants {
            let session = MockSession::new(peer(), ready_frame(fields));
            let observation = session.observation();
            assert!(matches!(
                qualify(session),
                Err(NativeShadowQualificationError::BindingMismatch { field, .. })
                    if field == expected_field
            ));
            assert!(!observation.borrow().shutdown);
        }
    }

    #[test]
    fn rejects_launcher_peer_and_ready_identity_mismatches() {
        let mut cases = Vec::new();
        let mut launcher_pid = ready_fields();
        launcher_pid.launcher_pid += 1;
        cases.push(("launcherPid", launcher_pid));
        let mut node_uid = ready_fields();
        node_uid.node_uid += 7;
        cases.push(("nodeUid", node_uid));
        let mut node_gid = ready_fields();
        node_gid.node_gid += 7;
        cases.push(("nodeGid", node_gid));
        let mut checker_uid = ready_fields();
        checker_uid.checker_uid += 7;
        cases.push(("checkerUid", checker_uid));
        let mut checker_gid = ready_fields();
        checker_gid.checker_gid += 7;
        cases.push(("checkerGid", checker_gid));

        for (expected_field, fields) in cases {
            let session = MockSession::new(peer(), ready_frame(fields));
            let observation = session.observation();
            assert!(matches!(
                qualify(session),
                Err(NativeShadowQualificationError::BindingMismatch { field, .. })
                    if field == expected_field
            ));
            assert!(!observation.borrow().shutdown);
        }
    }

    #[test]
    fn strict_ready_contract_rejects_false_readiness_or_nonzero_recovery_state() {
        for (field, value) in [
            ("activationAllowed", json!(true)),
            ("ready", json!(false)),
            ("startupRecoveryComplete", json!(false)),
            ("activeExecutionLeaves", json!(1)),
            ("unexpectedDirectCgroupChildren", json!(1)),
            ("managerSubgroupVerified", json!(false)),
        ] {
            let session =
                MockSession::new(peer(), raw_mutated_ready_frame(field, value)).allow_early_eof();
            let observation = session.observation();
            assert!(matches!(
                qualify(session),
                Err(NativeShadowQualificationError::Wire(_))
            ));
            assert!(!observation.borrow().shutdown);
        }
    }

    #[test]
    fn rejects_clean_or_partial_eof_before_ready() {
        let clean = MockSession::new(peer(), Vec::new()).allow_early_eof();
        assert_eq!(
            qualify(clean),
            Err(NativeShadowQualificationError::PrematureEof)
        );

        for response in [vec![0, 0], vec![0, 0, 0, 8, b'{', b'}']] {
            let partial = MockSession::new(peer(), response).allow_early_eof();
            assert!(matches!(
                qualify(partial),
                Err(NativeShadowQualificationError::Wire(_))
            ));
        }
    }

    #[test]
    fn rejects_oversized_second_or_trailing_response_bytes() {
        let mut oversized_header = ((MAX_RESPONSE_FRAME_BYTES + 1) as u32)
            .to_be_bytes()
            .to_vec();
        oversized_header.extend_from_slice(b"ignored");
        let oversized = MockSession::new(peer(), oversized_header).allow_early_eof();
        assert!(matches!(
            qualify(oversized),
            Err(NativeShadowQualificationError::Wire(_))
        ));

        let mut two_frames = valid_ready_frame();
        two_frames.extend_from_slice(&valid_ready_frame());
        let second = MockSession::new(peer(), two_frames);
        assert_eq!(
            qualify(second),
            Err(NativeShadowQualificationError::UnexpectedSecondReady)
        );

        let mut with_trailing_byte = valid_ready_frame();
        with_trailing_byte.push(0xff);
        let trailing = MockSession::new(peer(), with_trailing_byte);
        assert!(matches!(
            qualify(trailing),
            Err(NativeShadowQualificationError::Wire(_))
        ));
    }

    #[test]
    fn shutdown_failure_prevents_success_and_eof_acceptance() {
        let mut session = MockSession::new(peer(), valid_ready_frame());
        session.shutdown_fails = true;
        let observation = session.observation();

        assert!(matches!(
            qualify(session),
            Err(NativeShadowQualificationError::ShutdownWrite(_))
        ));
        let observation = observation.borrow();
        assert!(!observation.shutdown);
        assert_eq!(observation.events.last(), Some(&Event::ShutdownWrite));
    }

    #[test]
    fn missing_peer_eof_after_shutdown_prevents_success() {
        let mut session = MockSession::new(peer(), valid_ready_frame());
        session.post_shutdown_eof_fails = true;
        let observation = session.observation();

        assert!(matches!(
            qualify(session),
            Err(NativeShadowQualificationError::Wire(_))
        ));
        let observation = observation.borrow();
        assert!(observation.shutdown);
        assert_eq!(observation.events.last(), Some(&Event::ReadEof));
    }
}
