use std::io::{self, Read, Write};
use std::path::Path;

use boole_native_shadow_protocol::{
    read_qualification_ready, write_qualification_hello, QualificationHello, QualificationReady,
    VerifiedAuthorityBundle, WireError,
};
use thiserror::Error;

const FIXED_LAUNCHER_SOCKET_PATH: &str = "/run/boole/native-shadow/launcher.sock";
const CONNECT_TIMEOUT_MILLIS: u64 = 1_000;
const HANDSHAKE_TIMEOUT_MILLIS: u64 = 5_000;

fn fixed_launcher_socket_path() -> &'static Path {
    Path::new(FIXED_LAUNCHER_SOCKET_PATH)
}

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

#[derive(Debug, Error, PartialEq, Eq)]
enum NativeShadowStartupError {
    #[cfg(not(target_os = "linux"))]
    #[error("native-shadow qualification requires Linux")]
    UnsupportedPlatform,
    #[cfg(target_os = "linux")]
    #[error("installed authority verification failed: {0}")]
    Authority(String),
    #[cfg(target_os = "linux")]
    #[error("fixed service identity resolution failed: {0}")]
    Identity(String),
    #[cfg(target_os = "linux")]
    #[error("launcher socket connection failed: {0}")]
    Connect(String),
    #[error("getrandom(2) failed: {0}")]
    NonceSyscall(String),
    #[error("getrandom(2) returned {actual} bytes instead of exactly 32")]
    NonceShortRead { actual: usize },
    #[cfg(target_os = "linux")]
    #[error(transparent)]
    Qualification(#[from] NativeShadowQualificationError),
}

fn qualification_nonce_from_one_call<F>(
    call: F,
) -> Result<QualificationNonce, NativeShadowStartupError>
where
    F: FnOnce(&mut [u8; 32], u32) -> io::Result<usize>,
{
    let mut bytes = [0_u8; 32];
    let actual = call(&mut bytes, 0)
        .map_err(|error| NativeShadowStartupError::NonceSyscall(error.to_string()))?;
    if actual != bytes.len() {
        return Err(NativeShadowStartupError::NonceShortRead { actual });
    }
    Ok(QualificationNonce::from_bytes(bytes))
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

#[cfg(not(target_os = "linux"))]
fn qualify_installed_native_shadow_launcher(
) -> Result<NativeShadowQualificationReadiness, NativeShadowStartupError> {
    Err(NativeShadowStartupError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::{self, Read, Write};
    use std::mem::{self, MaybeUninit};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use boole_native_shadow_protocol::{
        installed_authority::open_verified_installed_authority_bundle,
        resolve_fixed_service_identities,
    };

    use super::{
        fixed_launcher_socket_path, qualification_nonce_from_one_call,
        qualify_native_shadow_launcher, NativeShadowExpectedIdentities,
        NativeShadowPeerCredentials, NativeShadowQualificationReadiness,
        NativeShadowQualificationSession, NativeShadowStartupError, CONNECT_TIMEOUT_MILLIS,
        HANDSHAKE_TIMEOUT_MILLIS,
    };

    struct UnixQualificationSession {
        stream: UnixStream,
        deadline: Instant,
    }

    impl UnixQualificationSession {
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

    impl Read for UnixQualificationSession {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.stream.set_read_timeout(Some(self.remaining()?))?;
            self.stream.read(buffer)
        }
    }

    impl Write for UnixQualificationSession {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.stream.set_write_timeout(Some(self.remaining()?))?;
            self.stream.write(buffer)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.stream.set_write_timeout(Some(self.remaining()?))?;
            self.stream.flush()
        }
    }

    impl NativeShadowQualificationSession for UnixQualificationSession {
        fn peer_credentials(&mut self) -> io::Result<NativeShadowPeerCredentials> {
            let _ = self.remaining()?;
            peer_credentials(&self.stream)
        }

        fn shutdown_write(&mut self) -> io::Result<()> {
            let _ = self.remaining()?;
            self.stream.shutdown(std::net::Shutdown::Write)
        }
    }

    pub(super) fn qualify_installed(
    ) -> Result<NativeShadowQualificationReadiness, NativeShadowStartupError> {
        let identities = resolve_fixed_service_identities()
            .map_err(|error| NativeShadowStartupError::Identity(error.to_string()))?;
        let expected = NativeShadowExpectedIdentities {
            node_uid: identities.node_uid(),
            node_gid: identities.node_gid(),
            checker_uid: identities.checker_uid(),
            checker_gid: identities.checker_gid(),
        }
        .validate()?;
        let authority = open_verified_installed_authority_bundle()
            .map_err(|error| NativeShadowStartupError::Authority(error.to_string()))?;
        let stream = connect_unix_with_timeout(
            fixed_launcher_socket_path(),
            Duration::from_millis(CONNECT_TIMEOUT_MILLIS),
        )
        .map_err(|error| NativeShadowStartupError::Connect(error.to_string()))?;
        let nonce = qualification_nonce_from_one_call(getrandom_once)?;
        let session =
            UnixQualificationSession::new(stream, Duration::from_millis(HANDSHAKE_TIMEOUT_MILLIS));
        qualify_native_shadow_launcher(session, &authority, nonce, expected).map_err(Into::into)
    }

    #[allow(unsafe_code)]
    fn getrandom_once(output: &mut [u8; 32], flags: u32) -> io::Result<usize> {
        // SAFETY: `output` is a live writable 32-byte buffer for the duration
        // of the syscall, and no alias is read while the kernel writes it.
        let result = unsafe {
            libc::getrandom(
                output.as_mut_ptr().cast::<libc::c_void>(),
                output.len(),
                flags,
            )
        };
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(result as usize)
        }
    }

    #[allow(unsafe_code)]
    fn peer_credentials(stream: &UnixStream) -> io::Result<NativeShadowPeerCredentials> {
        let mut credentials = MaybeUninit::<libc::ucred>::uninit();
        let mut length = mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: `credentials` points to writable storage of exactly `length`
        // bytes, and the descriptor belongs to a live Unix stream socket.
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
        // SAFETY: a successful `getsockopt` with the exact expected length
        // initialized every byte of `libc::ucred`.
        let credentials = unsafe { credentials.assume_init() };
        let pid = u32::try_from(credentials.pid).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "SO_PEERCRED PID is negative")
        })?;
        Ok(NativeShadowPeerCredentials {
            pid,
            uid: credentials.uid,
            gid: credentials.gid,
        })
    }

    #[allow(unsafe_code)]
    fn connect_unix_with_timeout(path: &Path, timeout: Duration) -> io::Result<UnixStream> {
        // SAFETY: `socket` has no pointer arguments and returns a new owned FD
        // on success. It is wrapped in `OwnedFd` immediately below.
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
        // SAFETY: `socket` returned one new descriptor and no other owner is
        // constructed from it.
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
            if !connect_is_pending(&error) {
                return Err(error);
            }
            wait_until_connected(descriptor.as_raw_fd(), timeout)?;
        }

        set_nonblocking(descriptor.as_raw_fd(), false)?;
        Ok(UnixStream::from(descriptor))
    }

    fn connect_is_pending(error: &io::Error) -> bool {
        matches!(
            error.raw_os_error(),
            Some(code) if code == libc::EINPROGRESS || code == libc::EAGAIN
        )
    }

    #[allow(unsafe_code)]
    fn unix_address(path: &Path) -> io::Result<(libc::sockaddr_un, libc::socklen_t)> {
        use std::os::unix::ffi::OsStrExt;

        let path = path.as_os_str().as_bytes();
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
            let milliseconds = remaining.as_millis().clamp(1, i32::MAX as u128) as i32;
            let mut descriptor = libc::pollfd {
                fd,
                events: libc::POLLOUT,
                revents: 0,
            };
            // SAFETY: `descriptor` points to one initialized `pollfd` for the
            // duration of this call.
            let result = unsafe { libc::poll(&mut descriptor, 1, milliseconds) };
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
            return socket_error(fd);
        }
    }

    #[allow(unsafe_code)]
    fn socket_error(fd: libc::c_int) -> io::Result<()> {
        let mut error = 0;
        let mut length = mem::size_of::<libc::c_int>() as libc::socklen_t;
        // SAFETY: `error` is writable storage of exactly `length` bytes and
        // `fd` is the still-owned socket descriptor.
        let result = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                (&mut error as *mut libc::c_int).cast::<libc::c_void>(),
                &mut length,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        if length as usize != mem::size_of::<libc::c_int>() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SO_ERROR returned an unexpected length",
            ));
        }
        if error == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(error))
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
        use std::io::{Read, Write};
        use std::os::fd::AsRawFd;
        use std::os::unix::net::{UnixListener, UnixStream};
        use std::path::PathBuf;
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::Duration;

        use super::{
            connect_is_pending, connect_unix_with_timeout, getrandom_once, peer_credentials,
            UnixQualificationSession,
        };
        use crate::native_shadow_qualification::{
            qualification_nonce_from_one_call, NativeShadowQualificationSession,
        };

        static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

        struct SocketPathGuard(PathBuf);

        impl Drop for SocketPathGuard {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }

        fn socket_path() -> PathBuf {
            let suffix = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
            std::env::temp_dir().join(format!(
                "boole-native-shadow-node-{}-{suffix}.sock",
                std::process::id()
            ))
        }

        #[test]
        fn linux_unix_connect_pending_errors_enter_the_bounded_poll_path() {
            assert!(connect_is_pending(&std::io::Error::from_raw_os_error(
                libc::EINPROGRESS
            )));
            assert!(connect_is_pending(&std::io::Error::from_raw_os_error(
                libc::EAGAIN
            )));
            assert!(!connect_is_pending(&std::io::Error::from_raw_os_error(
                libc::ECONNREFUSED
            )));
        }

        #[test]
        fn linux_nonce_comes_from_one_exact_getrandom_call() {
            let nonce = qualification_nonce_from_one_call(getrandom_once)
                .expect("Linux getrandom must return one exact nonce");
            assert_eq!(nonce.encode_hex().len(), 64);
        }

        #[test]
        #[allow(unsafe_code)]
        fn kernel_peer_credentials_and_shutdown_are_observed_on_the_real_stream() {
            let (stream, mut peer) = UnixStream::pair().expect("Unix stream pair");
            let credentials = peer_credentials(&stream).expect("SO_PEERCRED must succeed");
            assert_eq!(credentials.pid, std::process::id());
            // SAFETY: these zero-argument identity syscalls have no memory
            // safety preconditions.
            assert_eq!(credentials.uid, unsafe { libc::geteuid() });
            // SAFETY: see above.
            assert_eq!(credentials.gid, unsafe { libc::getegid() });

            let mut session = UnixQualificationSession::new(stream, Duration::from_secs(1));
            session.shutdown_write().expect("shutdown-write succeeds");
            let mut byte = [0_u8; 1];
            assert_eq!(peer.read(&mut byte).expect("peer observes EOF"), 0);
        }

        #[test]
        #[allow(unsafe_code)]
        fn private_connector_uses_a_real_unix_socket_and_returns_blocking_stream() {
            let path = socket_path();
            let _path_guard = SocketPathGuard(path.clone());
            let listener = UnixListener::bind(&path).expect("test listener bind");
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("test accept");
                let mut byte = [0_u8; 1];
                stream.read_exact(&mut byte).expect("test server read");
                byte[0]
            });

            let mut stream = connect_unix_with_timeout(&path, Duration::from_secs(1))
                .expect("private connector succeeds");
            // SAFETY: F_GETFL has no pointer arguments and `stream` owns a
            // live descriptor for this assertion.
            let flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFL) };
            assert!(flags >= 0);
            assert_eq!(flags & libc::O_NONBLOCK, 0);
            stream.write_all(&[0x5a]).expect("test client write");
            assert_eq!(server.join().expect("test server joins"), 0x5a);
        }

        #[test]
        fn expired_total_deadline_refuses_io_before_touching_the_stream() {
            let (stream, _peer) = UnixStream::pair().expect("Unix stream pair");
            let mut session = UnixQualificationSession::new(stream, Duration::ZERO);
            let mut byte = [0_u8; 1];
            assert_eq!(
                session
                    .read(&mut byte)
                    .expect_err("expired read must fail")
                    .kind(),
                std::io::ErrorKind::TimedOut
            );
        }
    }
}

#[cfg(target_os = "linux")]
fn qualify_installed_native_shadow_launcher(
) -> Result<NativeShadowQualificationReadiness, NativeShadowStartupError> {
    linux::qualify_installed()
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
        fixed_launcher_socket_path, qualification_nonce_from_one_call,
        qualify_installed_native_shadow_launcher, qualify_native_shadow_launcher,
        NativeShadowExpectedIdentities, NativeShadowPeerCredentials,
        NativeShadowQualificationError, NativeShadowQualificationSession, NativeShadowStartupError,
        QualificationNonce,
    };

    const NONCE: [u8; 32] = [0x42; 32];
    const LAUNCHER_INSTANCE_ID: &str =
        "abababababababababababababababababababababababababababababababab";

    #[test]
    fn production_adapter_has_one_literal_socket_path_and_one_nonce_call() {
        let _entrypoint: fn() -> Result<_, _> = qualify_installed_native_shadow_launcher;
        assert_eq!(
            fixed_launcher_socket_path(),
            std::path::Path::new("/run/boole/native-shadow/launcher.sock")
        );

        let calls = std::cell::Cell::new(0);
        let nonce = qualification_nonce_from_one_call(|output, flags| {
            calls.set(calls.get() + 1);
            assert_eq!(output.len(), 32);
            assert_eq!(flags, 0);
            output.fill(0x5a);
            Ok(output.len())
        })
        .expect("one exact 32-byte call must produce a nonce");

        assert_eq!(calls.get(), 1);
        assert_eq!(nonce.encode_hex(), "5a".repeat(32));
    }

    #[test]
    fn production_socket_nonce_and_deadlines_match_the_tracked_policy() {
        let policy: Value = serde_json::from_slice(TRACKED_EXECUTION_POLICY_BYTES)
            .expect("tracked execution policy must be JSON");
        assert_eq!(
            policy.pointer("/installation/socketPath"),
            Some(&json!(fixed_launcher_socket_path().to_string_lossy()))
        );
        assert_eq!(
            policy.pointer("/ipc/connectTimeoutMillis"),
            Some(&json!(super::CONNECT_TIMEOUT_MILLIS))
        );
        assert_eq!(
            policy.pointer("/ipc/handshakeTimeoutMillis"),
            Some(&json!(super::HANDSHAKE_TIMEOUT_MILLIS))
        );
        assert_eq!(policy.pointer("/ipc/nonceBytes"), Some(&json!(32)));
        assert_eq!(
            policy.pointer("/ipc/nonceSource"),
            Some(&json!("node-getrandom:32-bytes:no-fallback-per-connection"))
        );
    }

    #[test]
    fn nonce_source_has_no_short_read_or_error_fallback() {
        for returned in [0, 31] {
            assert!(matches!(
                qualification_nonce_from_one_call(|_, _| Ok(returned)),
                Err(NativeShadowStartupError::NonceShortRead { actual }) if actual == returned
            ));
        }
        assert!(matches!(
            qualification_nonce_from_one_call(|_, _| Err(io::Error::other("getrandom failed"))),
            Err(NativeShadowStartupError::NonceSyscall(_))
        ));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn production_adapter_refuses_non_linux_before_filesystem_or_socket_work() {
        assert!(matches!(
            qualify_installed_native_shadow_launcher(),
            Err(NativeShadowStartupError::UnsupportedPlatform)
        ));
    }

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
