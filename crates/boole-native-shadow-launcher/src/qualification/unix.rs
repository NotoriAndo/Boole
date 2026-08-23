use std::io::{self, Read, Write};
use std::mem::{self, MaybeUninit};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use super::{
    private, NodePeerCredentials, QualificationServerError, QualificationSession,
    VerifiedQualificationStartup,
};

const HANDSHAKE_TIMEOUT_MILLIS: u64 = 5_000;

/// Serve one already-connected Linux qualification stream.
///
/// This adapter neither binds nor accepts a socket. The opaque startup token
/// keeps it unusable until a later runtime slice has verified every frozen
/// launcher-startup prerequisite.
pub fn serve_connected_unix_qualification(
    stream: UnixStream,
    startup: &VerifiedQualificationStartup,
) -> Result<(), QualificationServerError> {
    serve_connected_unix_qualification_with_timeout(
        stream,
        startup,
        Duration::from_millis(HANDSHAKE_TIMEOUT_MILLIS),
    )
}

fn serve_connected_unix_qualification_with_timeout(
    stream: UnixStream,
    startup: &VerifiedQualificationStartup,
    timeout: Duration,
) -> Result<(), QualificationServerError> {
    super::serve_request_free_qualification(
        LinuxQualificationSession::new(stream, timeout),
        startup,
    )
}

struct LinuxQualificationSession {
    stream: UnixStream,
    deadline: Instant,
}

impl LinuxQualificationSession {
    fn new(stream: UnixStream, timeout: Duration) -> Self {
        Self {
            stream,
            deadline: Instant::now() + timeout,
        }
    }

    fn remaining(&self) -> io::Result<Duration> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "native-shadow qualification deadline elapsed",
            ));
        }
        Ok(remaining)
    }
}

impl Read for LinuxQualificationSession {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(self.remaining()?))?;
        self.stream.read(buffer)
    }
}

impl Write for LinuxQualificationSession {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.flush()
    }
}

impl private::Sealed for LinuxQualificationSession {}

impl QualificationSession for LinuxQualificationSession {
    fn peer_credentials(&mut self) -> io::Result<NodePeerCredentials> {
        let _ = self.remaining()?;
        peer_credentials(&self.stream)
    }

    fn shutdown_write(&mut self) -> io::Result<()> {
        let _ = self.remaining()?;
        self.stream.shutdown(std::net::Shutdown::Write)
    }
}

#[allow(unsafe_code)]
fn peer_credentials(stream: &UnixStream) -> io::Result<NodePeerCredentials> {
    let mut credentials = MaybeUninit::<libc::ucred>::uninit();
    let mut length = mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `credentials` is writable storage of exactly `length` bytes,
    // and the descriptor belongs to a live Unix stream socket.
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
    // initialized every byte of `libc::ucred`.
    let credentials = unsafe { credentials.assume_init() };
    let pid = u32::try_from(credentials.pid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "SO_PEERCRED PID is negative"))?;
    Ok(NodePeerCredentials {
        pid,
        uid: credentials.uid,
        gid: credentials.gid,
    })
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::num::NonZeroU32;
    use std::os::unix::net::UnixStream;
    use std::thread;
    use std::time::Duration;

    use boole_native_shadow_protocol::{
        encode_qualification_hello_frame, read_qualification_ready, verify_authority_bundle,
        QualificationHello, TRACKED_EXECUTION_POLICY_BYTES, TRACKED_REGISTRY_BYTES,
        TRACKED_TOOLCHAIN_IDENTITY_BYTES,
    };
    use serde_json::{json, Value};

    use super::{
        serve_connected_unix_qualification, serve_connected_unix_qualification_with_timeout,
        LinuxQualificationSession, HANDSHAKE_TIMEOUT_MILLIS,
    };
    use crate::qualification::{
        QualificationServerError, QualificationSession, VerifiedQualificationStartup,
    };

    const NONCE: &str = "abababababababababababababababababababababababababababababababab";

    #[allow(unsafe_code)]
    fn current_ids() -> (u32, u32) {
        // SAFETY: these libc calls take no pointers and only return the current
        // process credentials.
        unsafe { (libc::geteuid(), libc::getegid()) }
    }

    fn distinct_nonzero(value: u32) -> u32 {
        if value == 1 {
            2
        } else {
            1
        }
    }

    fn startup(node_uid: u32, node_gid: u32) -> VerifiedQualificationStartup {
        assert_ne!(node_uid, 0, "the real node peer test must be non-root");
        assert_ne!(node_gid, 0, "the real node peer group must be non-root");
        VerifiedQualificationStartup {
            authority: verify_authority_bundle(
                TRACKED_REGISTRY_BYTES,
                TRACKED_EXECUTION_POLICY_BYTES,
                TRACKED_TOOLCHAIN_IDENTITY_BYTES,
            )
            .expect("tracked authority verifies"),
            launcher_pid: NonZeroU32::new(std::process::id()).expect("test PID is non-zero"),
            launcher_instance_id: [0x5a; 32],
            node_uid,
            node_gid,
            checker_uid: distinct_nonzero(node_uid),
            checker_gid: distinct_nonzero(node_gid),
        }
    }

    fn hello(startup: &VerifiedQualificationStartup) -> QualificationHello {
        QualificationHello::try_new(
            NONCE.to_string(),
            startup.authority.execution_policy_digest().to_string(),
            startup.authority.toolchain_identity_digest().to_string(),
            startup.authority.registry_digest().to_string(),
        )
        .expect("test hello validates")
    }

    #[test]
    #[ignore = "the named Ubuntu gate runs this exact kernel socket test"]
    fn real_kernel_stream_round_trip_observes_peer_and_half_close() {
        let (node_uid, node_gid) = current_ids();
        let startup = startup(node_uid, node_gid);
        let request = encode_qualification_hello_frame(&hello(&startup)).expect("hello encodes");
        let (launcher, mut node) = UnixStream::pair().expect("Unix stream pair");

        let server = thread::spawn(move || serve_connected_unix_qualification(launcher, &startup));
        node.write_all(&request).expect("node writes hello");
        let ready = read_qualification_ready(&mut node)
            .expect("ready frame is valid")
            .expect("launcher writes one ready frame");
        assert_eq!(ready.nonce_hex(), NONCE);
        node.shutdown(std::net::Shutdown::Write)
            .expect("node closes its write half after validating ready");
        let mut trailing = [0_u8; 1];
        assert_eq!(node.read(&mut trailing).expect("node observes EOF"), 0);
        server
            .join()
            .expect("launcher thread does not panic")
            .expect("launcher accepts the real peer and clean EOF");
    }

    #[test]
    fn untrusted_kernel_peer_is_rejected_before_queued_hello_is_read() {
        let (node_uid, node_gid) = current_ids();
        let startup = startup(distinct_nonzero(node_uid), node_gid);
        let request = encode_qualification_hello_frame(&hello(&startup)).expect("hello encodes");
        let (launcher, mut node) = UnixStream::pair().expect("Unix stream pair");
        let mut inspection = launcher.try_clone().expect("clone launcher descriptor");
        node.write_all(&request)
            .expect("queue hello before serving");

        assert_eq!(
            serve_connected_unix_qualification_with_timeout(
                launcher,
                &startup,
                Duration::from_secs(1),
            ),
            Err(QualificationServerError::UntrustedNodePeer)
        );

        let mut preserved = vec![0_u8; request.len()];
        inspection
            .read_exact(&mut preserved)
            .expect("credential rejection leaves the hello unread");
        assert_eq!(preserved, request);
        node.set_nonblocking(true).expect("set nonblocking read");
        let mut output = [0_u8; 1];
        assert_eq!(
            node.read(&mut output)
                .expect_err("no ready bytes were written")
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn one_expired_absolute_deadline_blocks_every_session_operation() {
        let (launcher, _node) = UnixStream::pair().expect("Unix stream pair");
        let mut session = LinuxQualificationSession::new(launcher, Duration::ZERO);

        let assert_timed_out = |error: std::io::Error| {
            assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
            assert!(error.to_string().contains("deadline elapsed"));
        };
        assert_timed_out(
            session
                .peer_credentials()
                .expect_err("peer lookup times out"),
        );
        assert_timed_out(session.read(&mut [0_u8; 1]).expect_err("read times out"));
        assert_timed_out(session.write(&[0_u8; 1]).expect_err("write times out"));
        assert_timed_out(session.flush().expect_err("flush times out"));
        assert_timed_out(session.shutdown_write().expect_err("shutdown times out"));
    }

    #[test]
    fn unix_session_constants_and_peer_order_match_the_tracked_policy() {
        let policy: Value = serde_json::from_slice(TRACKED_EXECUTION_POLICY_BYTES)
            .expect("tracked execution policy is JSON");
        assert_eq!(
            policy.pointer("/ipc/transport"),
            Some(&json!("unix-stream"))
        );
        assert_eq!(
            policy.pointer("/ipc/peerCredentials"),
            Some(&json!("SO_PEERCRED"))
        );
        assert_eq!(
            policy.pointer("/ipc/peerCredentialChecks/launcherAcceptsUid"),
            Some(&json!("resolved-boole-node-uid-only"))
        );
        assert_eq!(
            policy.pointer("/ipc/peerCredentialChecks/launcherAcceptsGid"),
            Some(&json!("resolved-boole-node-primary-gid-only"))
        );
        assert_eq!(
            policy.pointer("/ipc/peerCredentialChecks/requirePeerPid"),
            Some(&json!(true))
        );
        assert_eq!(
            policy.pointer("/ipc/peerCredentialChecks/validateBeforeFirstFrame"),
            Some(&json!(true))
        );
        assert_eq!(
            policy.pointer("/ipc/handshakeTimeoutMillis"),
            Some(&json!(HANDSHAKE_TIMEOUT_MILLIS))
        );
    }
}
