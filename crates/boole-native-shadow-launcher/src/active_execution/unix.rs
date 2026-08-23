use std::io::{self, Read, Write};
use std::mem::{self, MaybeUninit};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use super::{
    serve_active_execution_session, ActiveExecutionContext, ActiveExecutionServerError,
    ActiveExecutionSession, ContainedCheckerExecutor, NodePeerCredentials, ReplayGrantCapability,
};

const EXECUTION_SESSION_TIMEOUT: Duration = Duration::from_millis(115_000);

pub(super) fn serve_connected_unix_execution<E: ContainedCheckerExecutor>(
    stream: UnixStream,
    context: &ActiveExecutionContext,
    executor: &mut E,
    replay_grant: &ReplayGrantCapability,
) -> Result<(), ActiveExecutionServerError> {
    serve_active_execution_session(
        LinuxActiveExecutionSession::new(stream, EXECUTION_SESSION_TIMEOUT),
        context,
        executor,
        Some(replay_grant),
    )
}

struct LinuxActiveExecutionSession {
    stream: UnixStream,
    deadline: Instant,
}

impl LinuxActiveExecutionSession {
    fn new(stream: UnixStream, timeout: Duration) -> Self {
        Self {
            stream,
            deadline: Instant::now() + timeout,
        }
    }

    fn remaining(&self) -> io::Result<Duration> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "native-shadow active execution session deadline elapsed",
            ))
        } else {
            Ok(remaining)
        }
    }

    fn read_exact_or_clean_eof(&mut self, buffer: &mut [u8]) -> io::Result<Option<()>> {
        let mut filled = 0;
        while filled < buffer.len() {
            self.stream.set_read_timeout(Some(self.remaining()?))?;
            match self.stream.read(&mut buffer[filled..]) {
                Ok(0) if filled == 0 => return Ok(None),
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "native-shadow frame ended before its declared length",
                    ));
                }
                Ok(read) => filled += read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(Some(()))
    }
}

impl Write for LinuxActiveExecutionSession {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.flush()
    }
}

impl ActiveExecutionSession for LinuxActiveExecutionSession {
    fn peer_credentials(&mut self) -> io::Result<NodePeerCredentials> {
        let _ = self.remaining()?;
        peer_credentials(&self.stream)
    }

    fn read_frame(&mut self, cap: usize) -> io::Result<Option<Vec<u8>>> {
        let mut header = [0_u8; 4];
        if self.read_exact_or_clean_eof(&mut header)?.is_none() {
            return Ok(None);
        }
        let declared = u32::from_be_bytes(header) as usize;
        if declared > cap {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("native-shadow frame payload exceeds cap {cap}: {declared}"),
            ));
        }
        let mut frame = Vec::with_capacity(declared + 4);
        frame.extend_from_slice(&header);
        let mut payload = vec![0_u8; declared];
        self.read_exact_or_clean_eof(&mut payload)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "native-shadow frame body is missing",
            )
        })?;
        frame.extend_from_slice(&payload);
        Ok(Some(frame))
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
    // SAFETY: `credentials` is exact writable output storage and `stream`
    // owns a live Unix-stream descriptor for the duration of the call.
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
    // SAFETY: successful getsockopt with exact length initialized the value.
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
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::net::UnixStream;
    use std::thread;
    use std::time::Duration;

    use boole_native_shadow_protocol::{
        encode_execution_request_frame, execution_request_digest_hex, read_active_execution_ready,
        read_execution_report, write_execution_hello, ExecutionHello, ExecutionReport,
        ExecutionRequest,
    };

    use super::{
        peer_credentials, serve_connected_unix_execution, ActiveExecutionContext,
        ActiveExecutionSession, ContainedCheckerExecutor, LinuxActiveExecutionSession,
        ReplayGrantCapability,
    };

    struct PrivateContainedTestExecutor {
        node_uid: u32,
        node_gid: u32,
        checker_uid: u32,
        checker_gid: u32,
    }

    impl ContainedCheckerExecutor for PrivateContainedTestExecutor {
        fn execute(
            &mut self,
            _request: &ExecutionRequest,
            exact_request_frame: &[u8],
        ) -> Result<ExecutionReport, String> {
            Ok(super::super::tests::report_with_identities(
                execution_request_digest_hex(exact_request_frame)
                    .map_err(|error| error.to_string())?,
                self.node_uid,
                self.node_gid,
                self.checker_uid,
                self.checker_gid,
            ))
        }
    }

    #[test]
    fn real_socket_peer_and_exact_frame_reader_are_kernel_backed() {
        let (launcher, mut node) = UnixStream::pair().expect("Unix socketpair");
        let payload = b"{}";
        node.write_all(&(payload.len() as u32).to_be_bytes())
            .expect("node writes frame header");
        node.write_all(payload).expect("node writes frame payload");
        node.shutdown(std::net::Shutdown::Write)
            .expect("node half-closes after one frame");

        let peer = peer_credentials(&launcher).expect("kernel peer credentials");
        assert_eq!(peer.pid, std::process::id());
        assert_ne!(peer.pid, 0);

        let mut session = LinuxActiveExecutionSession::new(launcher, Duration::from_secs(1));
        assert_eq!(
            session.read_frame(16).expect("one exact frame"),
            Some([&2_u32.to_be_bytes()[..], payload].concat())
        );
        assert_eq!(session.read_frame(16).expect("clean node EOF"), None);
    }

    #[test]
    fn real_linux_socketpair_runs_one_complete_private_active_session() {
        let identity = fs::metadata("/proc/self").expect("Linux procfs identity");
        let node_uid = identity.uid();
        let node_gid = identity.gid();
        assert_ne!(node_uid, 0, "the Linux harness must run as a non-root peer");
        assert_ne!(node_gid, 0, "the Linux harness must run as a non-root peer");
        let checker_uid = node_uid.checked_add(1).expect("test UID has headroom");
        let checker_gid = node_gid.checked_add(1).expect("test GID has headroom");
        let request = super::super::tests::request();
        let request_frame = encode_execution_request_frame(&request).expect("strict Execute frame");
        let hello = ExecutionHello::try_from_execution_request_frame(&request_frame)
            .expect("Hello binds exact Execute bytes");
        let (launcher, mut node) = UnixStream::pair().expect("real Unix socketpair");
        let node_exchange = thread::spawn(move || {
            write_execution_hello(&mut node, &hello).expect("node writes strict Hello");
            node.write_all(&request_frame)
                .expect("node writes exact Execute frame");
            node.shutdown(std::net::Shutdown::Write)
                .expect("node half-closes after one Execute");
            let ready = read_active_execution_ready(&mut node)
                .expect("node reads active Ready")
                .expect("launcher emits one active Ready");
            let report = read_execution_report(&mut node)
                .expect("node reads execution Report")
                .expect("launcher emits one execution Report");
            assert!(read_execution_report(&mut node)
                .expect("launcher clean EOF")
                .is_none());
            (ready, report)
        });

        let context =
            ActiveExecutionContext::for_test(4_242, node_uid, node_gid, checker_uid, checker_gid);
        let replay_grant = ReplayGrantCapability::for_test();
        serve_connected_unix_execution(
            launcher,
            &context,
            &mut PrivateContainedTestExecutor {
                node_uid,
                node_gid,
                checker_uid,
                checker_gid,
            },
            &replay_grant,
        )
        .expect("private active service completes one real socket exchange");
        let (ready, report) = node_exchange.join().expect("node thread completes");
        assert!(ready.ready());
        assert_eq!(
            report.checker_verdict(),
            Some(boole_native_shadow_protocol::CheckerVerdict::Accepted)
        );
        assert!(report.cleanup_complete());
    }
}
