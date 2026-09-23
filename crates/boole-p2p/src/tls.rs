//! TLS 1.3 with mutually pinned RFC 7250 Ed25519 raw public keys.
//! The trust decision is an explicit identity allowlist, not DNS, CA trust,
//! TOFU or wallet authority. rustls still verifies the real handshake signature.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::{AlwaysResolvesClientRawPublicKeys, Resumption};
use rustls::pki_types::{
    CertificateDer, PrivatePkcs8KeyDer, ServerName, SubjectPublicKeyInfoDer, UnixTime,
};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::server::{AlwaysResolvesServerRawPublicKeys, NoServerSessionStorage};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, DistinguishedName, ServerConfig,
    ServerConnection, SignatureScheme, StreamOwned,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use zeroize::Zeroizing;

use crate::{read_line_capped, Frame, FrameError, Transport, MAX_FRAME_BYTES};

const ALPN: &[u8] = b"boole-transport/1";
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const TLS_OVERHEAD_BUDGET: usize = 64 * 1024;
const MAX_PEERS: usize = 64;
// RFC 8410 Ed25519 SubjectPublicKeyInfo: algorithm OID 1.3.101.112,
// absent parameters, followed by exactly 32 public-key bytes.
const ED25519_SPKI_PREFIX: &[u8] = &[
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

fn tls_error(detail: impl ToString) -> FrameError {
    FrameError::Tls {
        detail: detail.to_string(),
    }
}

/// A transport identity, deliberately unrelated to reward/owner/session keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId([u8; 32]);

impl PeerId {
    pub fn from_hex(value: &str) -> Result<Self, FrameError> {
        if value.len() != 64
            || value
                .bytes()
                .any(|b| !b.is_ascii_digit() && !(b'a'..=b'f').contains(&b))
        {
            return Err(tls_error("peer key must be 32-byte lowercase hex"));
        }
        let mut bytes = [0; 32];
        hex::decode_to_slice(value, &mut bytes).map_err(tls_error)?;
        let key = ed25519_dalek::VerifyingKey::from_bytes(&bytes).map_err(tls_error)?;
        if key.is_weak() {
            return Err(tls_error("weak peer key"));
        }
        Ok(Self(bytes))
    }

    pub fn to_hex(self) -> String {
        hex::encode(self.0)
    }

    fn from_spki(bytes: &[u8]) -> Result<Self, FrameError> {
        if bytes.len() != ED25519_SPKI_PREFIX.len() + 32 || !bytes.starts_with(ED25519_SPKI_PREFIX)
        {
            return Err(tls_error("peer key is not canonical Ed25519 SPKI"));
        }
        Self::from_hex(&hex::encode(&bytes[ED25519_SPKI_PREFIX.len()..]))
    }
}

#[derive(Clone)]
pub struct TlsIdentity {
    id: PeerId,
    key: Arc<rustls::sign::CertifiedKey>,
}

impl std::fmt::Debug for TlsIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsIdentity")
            .field("peer_id", &self.id)
            .finish_non_exhaustive()
    }
}

impl TlsIdentity {
    pub fn generate_pkcs8() -> Result<Zeroizing<Vec<u8>>, FrameError> {
        let generated =
            ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
                .map_err(|_| tls_error("peer key generation failed"))?;
        Ok(Zeroizing::new(generated.as_ref().to_vec()))
    }

    pub fn from_pkcs8(bytes: &[u8]) -> Result<Self, FrameError> {
        if bytes.is_empty() || bytes.len() > 4096 {
            return Err(tls_error("invalid peer private-key length"));
        }
        let key = rustls::crypto::ring::default_provider()
            .key_provider
            .load_private_key(PrivatePkcs8KeyDer::from(bytes.to_vec()).into())
            .map_err(|_| tls_error("invalid Ed25519 peer private key"))?;
        let spki = key
            .public_key()
            .ok_or_else(|| tls_error("peer key has no public key"))?;
        let id = PeerId::from_spki(spki.as_ref())?;
        let certificate = CertificateDer::from(spki.as_ref().to_vec());
        Ok(Self {
            id,
            key: Arc::new(rustls::sign::CertifiedKey::new(vec![certificate], key)),
        })
    }

    pub fn peer_id(&self) -> PeerId {
        self.id
    }
}

#[derive(Debug)]
struct Pins(BTreeSet<PeerId>);

impl Pins {
    fn check(
        &self,
        entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
    ) -> Result<(), rustls::Error> {
        let id = PeerId::from_spki(entity.as_ref())
            .map_err(|_| rustls::Error::General("invalid peer raw key".into()))?;
        if !intermediates.is_empty() || !self.0.contains(&id) {
            return Err(rustls::Error::General("unapproved peer key".into()));
        }
        Ok(())
    }
}

fn verify_signature(
    message: &[u8],
    key: &CertificateDer<'_>,
    signature: &DigitallySignedStruct,
) -> Result<HandshakeSignatureValid, rustls::Error> {
    if signature.scheme != SignatureScheme::ED25519 {
        return Err(rustls::Error::General(
            "Ed25519 handshake signature required".into(),
        ));
    }
    rustls::crypto::verify_tls13_signature_with_raw_key(
        message,
        &SubjectPublicKeyInfoDer::from(key.as_ref()),
        signature,
        &rustls::crypto::ring::default_provider().signature_verification_algorithms,
    )
}

impl ServerCertVerifier for Pins {
    fn verify_server_cert(
        &self,
        entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        ocsp: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        self.check(entity, intermediates)?;
        if !ocsp.is_empty() {
            return Err(rustls::Error::General("unexpected raw-key OCSP".into()));
        }
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is disabled".into()))
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        key: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_signature(message, key, signature)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}

impl ClientCertVerifier for Pins {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        self.check(entity, intermediates)?;
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is disabled".into()))
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        key: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_signature(message, key, signature)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}

/// An explicit finite membership and endpoint policy, fixed for this process.
/// A new transport/restart is required to change pins. Sessions cannot resume.
#[derive(Clone)]
pub struct TlsTransport {
    identity: TlsIdentity,
    server: Arc<ServerConfig>,
    endpoints: BTreeMap<SocketAddr, PeerId>,
}

impl TlsTransport {
    pub fn new(
        identity: TlsIdentity,
        peers: Vec<(SocketAddr, PeerId)>,
    ) -> Result<Self, FrameError> {
        if peers.len() > MAX_PEERS {
            return Err(tls_error("too many pinned peers"));
        }
        let mut endpoints = BTreeMap::new();
        let mut allowed = BTreeSet::new();
        for (address, id) in peers {
            if id == identity.id
                || address.port() == 0
                || address.ip().is_unspecified()
                || address.ip().is_multicast()
                || endpoints.insert(address, id).is_some()
                || !allowed.insert(id)
            {
                return Err(tls_error("invalid, self or duplicate peer configuration"));
            }
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut server = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(tls_error)?
            .with_client_cert_verifier(Arc::new(Pins(allowed)))
            .with_cert_resolver(Arc::new(AlwaysResolvesServerRawPublicKeys::new(
                identity.key.clone(),
            )));
        server.alpn_protocols = vec![ALPN.to_vec()];
        server.max_early_data_size = 0;
        server.send_tls13_tickets = 0;
        server.session_storage = Arc::new(NoServerSessionStorage {});
        Ok(Self {
            identity,
            server: Arc::new(server),
            endpoints,
        })
    }

    pub fn peer_id(&self) -> PeerId {
        self.identity.id
    }

    pub fn connect_until(
        &self,
        address: &SocketAddr,
        deadline: Instant,
    ) -> Result<TlsConn, FrameError> {
        if !self.endpoints.contains_key(address) {
            return Err(tls_error("endpoint has no pinned peer key"));
        }
        let socket = TcpStream::connect_timeout(address, remaining(deadline)?)?;
        self.connect_stream_until(socket, deadline)
    }

    /// Allows a lifecycle owner to register the socket before TLS starts.
    pub fn connect_stream_until(
        &self,
        socket: TcpStream,
        deadline: Instant,
    ) -> Result<TlsConn, FrameError> {
        socket.set_nonblocking(false)?;
        let address = socket.peer_addr()?;
        let expected = self
            .endpoints
            .get(&address)
            .ok_or_else(|| tls_error("endpoint has no pinned peer key"))?;
        let mut config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .map_err(tls_error)?
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(Pins(BTreeSet::from([*expected]))))
                .with_client_cert_resolver(Arc::new(AlwaysResolvesClientRawPublicKeys::new(
                    self.identity.key.clone(),
                )));
        config.alpn_protocols = vec![ALPN.to_vec()];
        config.enable_sni = false;
        config.enable_early_data = false;
        config.resumption = Resumption::disabled();
        socket.set_nodelay(true)?;
        let mut io = DeadlineIo::new(socket, deadline, TLS_OVERHEAD_BUDGET);
        let mut connection =
            ClientConnection::new(Arc::new(config), ServerName::from(address.ip()))
                .map_err(tls_error)?;
        connection.set_buffer_limit(Some(TLS_OVERHEAD_BUDGET));
        while connection.is_handshaking() {
            connection.complete_io(&mut io)?;
        }
        let peer = authenticated_peer(&connection)?;
        Ok(TlsConn {
            reader: BufReader::new(TlsStream::Client(Box::new(StreamOwned::new(
                connection, io,
            )))),
            peer,
            failed: false,
        })
    }

    /// Admission/concurrency owners call this only after acquiring a worker.
    /// A slow TLS handshake shares one absolute read+write deadline and budget.
    pub fn accept_stream_until(
        &self,
        socket: TcpStream,
        deadline: Instant,
    ) -> Result<TlsConn, FrameError> {
        // accept() can inherit O_NONBLOCK on macOS. This transport owns
        // blocking I/O with absolute deadlines, independent of the listener.
        socket.set_nonblocking(false)?;
        socket.set_nodelay(true)?;
        let mut io = DeadlineIo::new(socket, deadline, TLS_OVERHEAD_BUDGET);
        let mut connection = ServerConnection::new(self.server.clone()).map_err(tls_error)?;
        connection.set_buffer_limit(Some(TLS_OVERHEAD_BUDGET));
        while connection.is_handshaking() {
            connection.complete_io(&mut io)?;
        }
        let peer = authenticated_peer(&connection)?;
        Ok(TlsConn {
            reader: BufReader::new(TlsStream::Server(Box::new(StreamOwned::new(
                connection, io,
            )))),
            peer,
            failed: false,
        })
    }

    pub fn recv_frame_counted_until(
        &self,
        conn: &mut TlsConn,
        remaining_wire_bytes: usize,
        deadline: Instant,
    ) -> Result<(Frame, usize), FrameError> {
        let (frame, count): (Frame, usize) =
            self.recv_json_counted_until(conn, remaining_wire_bytes, deadline)?;
        if let Err(error) = frame.validate() {
            conn.failed = true;
            return Err(error);
        }
        Ok((frame, count))
    }

    /// Typed messages use the same encrypted, bounded newline codec. Parsing
    /// directly into the message type preserves duplicate/unknown-field errors
    /// rather than losing them through an intermediate JSON Value.
    pub fn recv_json_counted_until<T: DeserializeOwned>(
        &self,
        conn: &mut TlsConn,
        remaining_wire_bytes: usize,
        deadline: Instant,
    ) -> Result<(T, usize), FrameError> {
        let cap = remaining_wire_bytes.min(MAX_FRAME_BYTES);
        let result = (|| {
            conn.begin(deadline, cap)?;
            if cap == 0 {
                return Err(FrameError::FrameBudgetExceeded { cap, seen: 0 });
            }
            let line = match read_line_capped(&mut conn.reader, cap) {
                Err(FrameError::FrameTooLarge { seen }) if cap < MAX_FRAME_BYTES => {
                    return Err(FrameError::FrameBudgetExceeded { cap, seen })
                }
                value => value?,
            };
            remaining(deadline)?;
            let count = line.len() + 1;
            let message = serde_json::from_slice(&line).map_err(|error| FrameError::Malformed {
                detail: error.to_string(),
            })?;
            Ok((message, count))
        })();
        if result.is_err() {
            conn.failed = true;
        }
        result
    }

    pub fn send_frame_until(
        &self,
        conn: &mut TlsConn,
        frame: &Frame,
        deadline: Instant,
    ) -> Result<(), FrameError> {
        if let Err(error) = frame.validate() {
            conn.failed = true;
            return Err(error);
        }
        self.send_json_counted_until(conn, frame, MAX_FRAME_BYTES, deadline)
            .map(|_| ())
    }

    pub fn send_json_counted_until<T: Serialize + ?Sized>(
        &self,
        conn: &mut TlsConn,
        message: &T,
        maximum_wire_bytes: usize,
        deadline: Instant,
    ) -> Result<usize, FrameError> {
        let result = (|| {
            let mut encoded =
                serde_json::to_vec(message).map_err(|error| FrameError::Malformed {
                    detail: error.to_string(),
                })?;
            encoded.push(b'\n');
            let cap = maximum_wire_bytes.min(MAX_FRAME_BYTES);
            if encoded.len() > cap {
                return Err(FrameError::FrameBudgetExceeded {
                    cap,
                    seen: encoded.len(),
                });
            }
            conn.begin(deadline, encoded.len())?;
            conn.reader.get_mut().write_all(&encoded)?;
            conn.reader.get_mut().flush()?;
            remaining(deadline)?;
            Ok(encoded.len())
        })();
        if result.is_err() {
            conn.failed = true;
        }
        result
    }
}

fn authenticated_peer(state: &rustls::CommonState) -> Result<PeerId, FrameError> {
    if state.alpn_protocol() != Some(ALPN)
        || state.protocol_version() != Some(rustls::ProtocolVersion::TLSv1_3)
    {
        return Err(tls_error("TLS 1.3 and Boole ALPN required"));
    }
    let certificates = state
        .peer_certificates()
        .ok_or_else(|| tls_error("mutual peer identity required"))?;
    if certificates.len() != 1 {
        return Err(tls_error("exactly one raw peer key required"));
    }
    PeerId::from_spki(certificates[0].as_ref())
}

pub struct TlsConn {
    reader: BufReader<TlsStream>,
    peer: PeerId,
    failed: bool,
}

impl TlsConn {
    pub fn peer_id(&self) -> PeerId {
        self.peer
    }

    fn begin(&mut self, deadline: Instant, plaintext_budget: usize) -> Result<(), FrameError> {
        if self.failed {
            return Err(tls_error("failed TLS connection cannot be reused"));
        }
        remaining(deadline)?;
        let io = self.reader.get_mut().io_mut();
        io.deadline = deadline;
        io.remaining_bytes = plaintext_budget.saturating_add(TLS_OVERHEAD_BUDGET);
        Ok(())
    }
}

impl Transport for TlsTransport {
    type Conn = TlsConn;
    type Listener = TcpListener;
    fn bind(&self, address: &str) -> Result<TcpListener, FrameError> {
        let address: SocketAddr = address
            .parse()
            .map_err(|_| tls_error("numeric socket address required"))?;
        Ok(TcpListener::bind(address)?)
    }
    fn local_addr(&self, listener: &TcpListener) -> Result<SocketAddr, FrameError> {
        Ok(listener.local_addr()?)
    }
    fn accept(&self, listener: &mut TcpListener) -> Result<(TlsConn, SocketAddr), FrameError> {
        let (socket, address) = listener.accept()?;
        Ok((
            self.accept_stream_until(socket, Instant::now() + IO_TIMEOUT)?,
            address,
        ))
    }
    fn connect(&self, address: &SocketAddr) -> Result<TlsConn, FrameError> {
        self.connect_until(address, Instant::now() + IO_TIMEOUT)
    }
    fn send_frame(&self, conn: &mut TlsConn, frame: &Frame) -> Result<(), FrameError> {
        self.send_frame_until(conn, frame, Instant::now() + IO_TIMEOUT)
    }
    fn recv_frame(&self, conn: &mut TlsConn) -> Result<Frame, FrameError> {
        self.recv_frame_counted_until(conn, MAX_FRAME_BYTES, Instant::now() + IO_TIMEOUT)
            .map(|(frame, _)| frame)
    }
}

enum TlsStream {
    Client(Box<StreamOwned<ClientConnection, DeadlineIo>>),
    Server(Box<StreamOwned<ServerConnection, DeadlineIo>>),
}

impl TlsStream {
    fn io_mut(&mut self) -> &mut DeadlineIo {
        match self {
            Self::Client(stream) => &mut stream.sock,
            Self::Server(stream) => &mut stream.sock,
        }
    }
}
impl Read for TlsStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Client(stream) => stream.read(bytes),
            Self::Server(stream) => stream.read(bytes),
        }
    }
}
impl Write for TlsStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Self::Client(stream) => stream.write(bytes),
            Self::Server(stream) => stream.write(bytes),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Client(stream) => stream.flush(),
            Self::Server(stream) => stream.flush(),
        }
    }
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "absolute TLS deadline exceeded"))
}

/// The deadline is below rustls, so a partial TLS record/handshake cannot
/// repeatedly renew a socket timeout while rustls is assembling it.
struct DeadlineIo {
    socket: TcpStream,
    deadline: Instant,
    remaining_bytes: usize,
}
impl DeadlineIo {
    fn new(socket: TcpStream, deadline: Instant, remaining_bytes: usize) -> Self {
        Self {
            socket,
            deadline,
            remaining_bytes,
        }
    }
    fn allowance(&self, length: usize) -> io::Result<usize> {
        remaining(self.deadline)?;
        if self.remaining_bytes == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "TLS wire budget exhausted",
            ));
        }
        Ok(length.min(self.remaining_bytes))
    }
}
impl Read for DeadlineIo {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let length = self.allowance(bytes.len())?;
        self.socket.set_read_timeout(Some(
            remaining(self.deadline)?.max(Duration::from_millis(1)),
        ))?;
        let count = self.socket.read(&mut bytes[..length])?;
        self.remaining_bytes -= count;
        remaining(self.deadline)?;
        Ok(count)
    }
}
impl Write for DeadlineIo {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let length = self.allowance(bytes.len())?;
        self.socket.set_write_timeout(Some(
            remaining(self.deadline)?.max(Duration::from_millis(1)),
        ))?;
        let count = self.socket.write(&bytes[..length])?;
        self.remaining_bytes -= count;
        remaining(self.deadline)?;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        remaining(self.deadline)?;
        self.socket.flush()
    }
}
