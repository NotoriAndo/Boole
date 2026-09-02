//! Fixed-size MAC.4 host/guest liveness and boot-binding handshake.
//!
//! Authentication is deliberately split across layers. The kernel-reported
//! AF_VSOCK peer CID authenticates the Mac host to the guest. On the Mac, the
//! `VZVirtioSocketDevice` used to connect is owned by the exact VM whose boot
//! files were verified before start. This frame adds freshness, protocol
//! versioning and an exact boot-tuple binding without inventing a second
//! challenge, replay, journal or verdict authority inside the guest.

use std::cell::RefCell;
use std::fmt;
use std::io::{Read, Write};

pub const VSOCK_PORT: u32 = 4050;
pub const PROXY_VSOCK_PORT: u32 = 4051;
pub const HOST_CID: u32 = 2;
pub const FRAME_BYTES: usize = 108;
pub const PROXY_FRAME_BYTES: usize = 120;
pub const PROXY_REQUEST_FRAME_CAP_BYTES: usize = 131_072;
pub const PROXY_RESPONSE_FRAME_CAP_BYTES: usize = 65_536;
pub const CONTRACT_SHA256_HEX: &str =
    "4f2ec110d72f628207ac383668daff7bda6b568449fd315d8376aeb20ae08bbd";
pub const PROXY_CONTRACT_SHA256_HEX: &str =
    "74d2f8c0be187a0b3ff0c9a1272bd5cef6943222448b4c6e7f7a97f209763613";

const MAGIC: [u8; 8] = *b"BOOLE4V1";
const HELLO_KIND: u8 = 1;
const READY_KIND: u8 = 2;
const PROXY_MAGIC: [u8; 8] = *b"BOOLE4P1";
const PROXY_OPEN_KIND: u8 = 1;
const PROXY_READY_KIND: u8 = 2;
const PROTOCOL_BINDING: [u8; 32] = [
    0x4f, 0x2e, 0xc1, 0x10, 0xd7, 0x2f, 0x62, 0x82, 0x07, 0xac, 0x38, 0x36, 0x68, 0xda, 0xff, 0x7b,
    0xda, 0x6b, 0x56, 0x84, 0x49, 0xfd, 0x31, 0x5d, 0x83, 0x76, 0xae, 0xb2, 0x0a, 0xe0, 0x8b, 0xbd,
];
const PROXY_PROTOCOL_BINDING: [u8; 32] = [
    0x74, 0xd2, 0xf8, 0xc0, 0xbe, 0x18, 0x7a, 0x0b, 0x3f, 0xf0, 0xc9, 0xa1, 0x27, 0x2b, 0xd5, 0xce,
    0xf6, 0x94, 0x32, 0x22, 0x44, 0x8b, 0x4c, 0x6e, 0x7f, 0x7a, 0x97, 0xf2, 0x09, 0x76, 0x36, 0x13,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameError(&'static str);

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for FrameError {}

#[derive(Debug)]
pub struct ProxyRelayError(String);

impl fmt::Display for ProxyRelayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ProxyRelayError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostHello {
    nonce: [u8; 32],
    boot_tuple_binding: [u8; 32],
}

impl HostHello {
    pub fn new(nonce: [u8; 32], boot_tuple_binding: [u8; 32]) -> Result<Self, FrameError> {
        require_nonzero(&nonce, "host nonce must not be all zero")?;
        require_nonzero(
            &boot_tuple_binding,
            "boot tuple binding must not be all zero",
        )?;
        Ok(Self {
            nonce,
            boot_tuple_binding,
        })
    }

    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }

    pub fn boot_tuple_binding(&self) -> &[u8; 32] {
        &self.boot_tuple_binding
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestReady {
    nonce: [u8; 32],
    boot_tuple_binding: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ProxyPhase {
    Qualification = 1,
    Execution = 2,
}

impl ProxyPhase {
    fn try_from_byte(value: u8) -> Result<Self, FrameError> {
        match value {
            1 => Ok(Self::Qualification),
            2 => Ok(Self::Execution),
            _ => Err(FrameError("proxy phase differs")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostProxyHello {
    nonce: [u8; 32],
    boot_tuple_binding: [u8; 32],
    phase: ProxyPhase,
}

impl HostProxyHello {
    pub fn new(
        nonce: [u8; 32],
        boot_tuple_binding: [u8; 32],
        phase: ProxyPhase,
    ) -> Result<Self, FrameError> {
        require_nonzero(&nonce, "proxy nonce must not be all zero")?;
        require_nonzero(
            &boot_tuple_binding,
            "proxy boot tuple binding must not be all zero",
        )?;
        Ok(Self {
            nonce,
            boot_tuple_binding,
            phase,
        })
    }

    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }

    pub fn boot_tuple_binding(&self) -> &[u8; 32] {
        &self.boot_tuple_binding
    }

    pub fn phase(&self) -> ProxyPhase {
        self.phase
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestProxyReady {
    nonce: [u8; 32],
    boot_tuple_binding: [u8; 32],
    phase: ProxyPhase,
    launcher_pid: u32,
    launcher_uid: u32,
    launcher_gid: u32,
}

struct DecodedProxyFrame {
    phase: ProxyPhase,
    nonce: [u8; 32],
    boot: [u8; 32],
    launcher_pid: u32,
    launcher_uid: u32,
    launcher_gid: u32,
}

impl GuestProxyReady {
    pub fn for_hello(
        hello: &HostProxyHello,
        launcher_pid: u32,
        launcher_uid: u32,
        launcher_gid: u32,
    ) -> Result<Self, FrameError> {
        if launcher_pid == 0 || launcher_uid != 0 || launcher_gid != 0 {
            return Err(FrameError("proxy launcher peer is not the root supervisor"));
        }
        Ok(Self {
            nonce: *hello.nonce(),
            boot_tuple_binding: *hello.boot_tuple_binding(),
            phase: hello.phase(),
            launcher_pid,
            launcher_uid,
            launcher_gid,
        })
    }

    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }

    pub fn boot_tuple_binding(&self) -> &[u8; 32] {
        &self.boot_tuple_binding
    }

    pub fn phase(&self) -> ProxyPhase {
        self.phase
    }

    pub fn launcher_pid(&self) -> u32 {
        self.launcher_pid
    }

    pub fn launcher_uid(&self) -> u32 {
        self.launcher_uid
    }

    pub fn launcher_gid(&self) -> u32 {
        self.launcher_gid
    }
}

impl GuestReady {
    pub fn for_hello(hello: &HostHello) -> Self {
        Self {
            nonce: *hello.nonce(),
            boot_tuple_binding: *hello.boot_tuple_binding(),
        }
    }

    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }

    pub fn boot_tuple_binding(&self) -> &[u8; 32] {
        &self.boot_tuple_binding
    }
}

fn require_nonzero(bytes: &[u8; 32], message: &'static str) -> Result<(), FrameError> {
    if bytes.iter().all(|byte| *byte == 0) {
        return Err(FrameError(message));
    }
    Ok(())
}

fn encode(kind: u8, nonce: &[u8; 32], boot: &[u8; 32]) -> [u8; FRAME_BYTES] {
    let mut frame = [0_u8; FRAME_BYTES];
    frame[..8].copy_from_slice(&MAGIC);
    frame[8] = kind;
    frame[12..44].copy_from_slice(&PROTOCOL_BINDING);
    frame[44..76].copy_from_slice(nonce);
    frame[76..108].copy_from_slice(boot);
    frame
}

fn decode(frame: &[u8], expected_kind: u8) -> Result<([u8; 32], [u8; 32]), FrameError> {
    if frame.len() != FRAME_BYTES {
        return Err(FrameError("frame length differs"));
    }
    if frame[..8] != MAGIC {
        return Err(FrameError("frame magic differs"));
    }
    if frame[8] != expected_kind {
        return Err(FrameError("frame kind differs"));
    }
    if frame[9..12] != [0, 0, 0] {
        return Err(FrameError("reserved frame bytes are nonzero"));
    }
    if frame[12..44] != PROTOCOL_BINDING {
        return Err(FrameError("protocol contract binding differs"));
    }
    let mut nonce = [0_u8; 32];
    nonce.copy_from_slice(&frame[44..76]);
    require_nonzero(&nonce, "host nonce must not be all zero")?;
    let mut boot = [0_u8; 32];
    boot.copy_from_slice(&frame[76..108]);
    require_nonzero(&boot, "boot tuple binding must not be all zero")?;
    Ok((nonce, boot))
}

pub fn encode_hello(hello: &HostHello) -> [u8; FRAME_BYTES] {
    encode(HELLO_KIND, hello.nonce(), hello.boot_tuple_binding())
}

pub fn decode_hello(frame: &[u8]) -> Result<HostHello, FrameError> {
    let (nonce, boot) = decode(frame, HELLO_KIND)?;
    HostHello::new(nonce, boot)
}

pub fn encode_ready(ready: &GuestReady) -> [u8; FRAME_BYTES] {
    encode(READY_KIND, ready.nonce(), ready.boot_tuple_binding())
}

pub fn decode_ready(frame: &[u8]) -> Result<GuestReady, FrameError> {
    let (nonce, boot_tuple_binding) = decode(frame, READY_KIND)?;
    Ok(GuestReady {
        nonce,
        boot_tuple_binding,
    })
}

pub fn validate_ready(expected: &HostHello, ready: &GuestReady) -> Result<(), FrameError> {
    if ready.nonce() != expected.nonce() {
        return Err(FrameError("guest response belongs to another attempt"));
    }
    if ready.boot_tuple_binding() != expected.boot_tuple_binding() {
        return Err(FrameError("guest response belongs to another boot tuple"));
    }
    Ok(())
}

fn encode_proxy(
    kind: u8,
    phase: ProxyPhase,
    nonce: &[u8; 32],
    boot: &[u8; 32],
    launcher_peer: (u32, u32, u32),
) -> [u8; PROXY_FRAME_BYTES] {
    let mut frame = [0_u8; PROXY_FRAME_BYTES];
    frame[..8].copy_from_slice(&PROXY_MAGIC);
    frame[8] = kind;
    frame[9] = phase as u8;
    frame[12..44].copy_from_slice(&PROXY_PROTOCOL_BINDING);
    frame[44..76].copy_from_slice(nonce);
    frame[76..108].copy_from_slice(boot);
    frame[108..112].copy_from_slice(&launcher_peer.0.to_be_bytes());
    frame[112..116].copy_from_slice(&launcher_peer.1.to_be_bytes());
    frame[116..120].copy_from_slice(&launcher_peer.2.to_be_bytes());
    frame
}

fn decode_proxy(
    frame: &[u8],
    expected_kind: u8,
) -> Result<DecodedProxyFrame, FrameError> {
    if frame.len() != PROXY_FRAME_BYTES {
        return Err(FrameError("proxy frame length differs"));
    }
    if frame[..8] != PROXY_MAGIC || frame[8] != expected_kind {
        return Err(FrameError("proxy frame identity differs"));
    }
    let phase = ProxyPhase::try_from_byte(frame[9])?;
    if frame[10..12] != [0, 0] || frame[12..44] != PROXY_PROTOCOL_BINDING {
        return Err(FrameError(
            "proxy reserved bytes or contract binding differs",
        ));
    }
    let mut nonce = [0_u8; 32];
    nonce.copy_from_slice(&frame[44..76]);
    require_nonzero(&nonce, "proxy nonce must not be all zero")?;
    let mut boot = [0_u8; 32];
    boot.copy_from_slice(&frame[76..108]);
    require_nonzero(&boot, "proxy boot tuple binding must not be all zero")?;
    let launcher_pid = u32::from_be_bytes(frame[108..112].try_into().expect("fixed range"));
    let launcher_uid = u32::from_be_bytes(frame[112..116].try_into().expect("fixed range"));
    let launcher_gid = u32::from_be_bytes(frame[116..120].try_into().expect("fixed range"));
    Ok(DecodedProxyFrame {
        phase,
        nonce,
        boot,
        launcher_pid,
        launcher_uid,
        launcher_gid,
    })
}

pub fn encode_proxy_hello(hello: &HostProxyHello) -> [u8; PROXY_FRAME_BYTES] {
    encode_proxy(
        PROXY_OPEN_KIND,
        hello.phase(),
        hello.nonce(),
        hello.boot_tuple_binding(),
        (0, 0, 0),
    )
}

pub fn decode_proxy_hello(frame: &[u8]) -> Result<HostProxyHello, FrameError> {
    let decoded = decode_proxy(frame, PROXY_OPEN_KIND)?;
    if (
        decoded.launcher_pid,
        decoded.launcher_uid,
        decoded.launcher_gid,
    ) != (0, 0, 0)
    {
        return Err(FrameError("host proxy frame forged launcher credentials"));
    }
    HostProxyHello::new(decoded.nonce, decoded.boot, decoded.phase)
}

pub fn encode_proxy_ready(ready: &GuestProxyReady) -> [u8; PROXY_FRAME_BYTES] {
    encode_proxy(
        PROXY_READY_KIND,
        ready.phase(),
        ready.nonce(),
        ready.boot_tuple_binding(),
        (
            ready.launcher_pid(),
            ready.launcher_uid(),
            ready.launcher_gid(),
        ),
    )
}

pub fn decode_proxy_ready(frame: &[u8]) -> Result<GuestProxyReady, FrameError> {
    let decoded = decode_proxy(frame, PROXY_READY_KIND)?;
    let hello = HostProxyHello::new(decoded.nonce, decoded.boot, decoded.phase)?;
    GuestProxyReady::for_hello(
        &hello,
        decoded.launcher_pid,
        decoded.launcher_uid,
        decoded.launcher_gid,
    )
}

pub fn validate_proxy_ready(
    expected: &HostProxyHello,
    ready: &GuestProxyReady,
) -> Result<(), FrameError> {
    if ready.nonce() != expected.nonce()
        || ready.boot_tuple_binding() != expected.boot_tuple_binding()
        || ready.phase() != expected.phase()
    {
        return Err(FrameError("proxy response belongs to another bound phase"));
    }
    if ready.launcher_pid() == 0 || ready.launcher_uid() != 0 || ready.launcher_gid() != 0 {
        return Err(FrameError("proxy response launcher peer is not root"));
    }
    Ok(())
}

fn read_bounded_frame<R: Read>(
    reader: &mut R,
    cap: usize,
    label: &'static str,
) -> Result<Vec<u8>, ProxyRelayError> {
    let mut header = [0_u8; 4];
    reader
        .read_exact(&mut header)
        .map_err(|error| ProxyRelayError(format!("read {label} header: {error}")))?;
    let declared = u32::from_be_bytes(header) as usize;
    if declared > cap {
        return Err(ProxyRelayError(format!(
            "{label} payload exceeds {cap} bytes"
        )));
    }
    let mut frame = Vec::with_capacity(4 + declared);
    frame.extend_from_slice(&header);
    frame.resize(4 + declared, 0);
    reader
        .read_exact(&mut frame[4..])
        .map_err(|error| ProxyRelayError(format!("read {label} body: {error}")))?;
    Ok(frame)
}

fn write_frame<W: Write>(
    writer: &mut W,
    frame: &[u8],
    label: &'static str,
) -> Result<(), ProxyRelayError> {
    writer
        .write_all(frame)
        .and_then(|()| writer.flush())
        .map_err(|error| ProxyRelayError(format!("write {label}: {error}")))
}

fn require_eof<R: Read>(reader: &mut R, label: &'static str) -> Result<(), ProxyRelayError> {
    let mut trailing = [0_u8; 1];
    match reader.read(&mut trailing) {
        Ok(0) => Ok(()),
        Ok(_) => Err(ProxyRelayError(format!("{label} has trailing bytes"))),
        Err(error) => Err(ProxyRelayError(format!("read {label} EOF: {error}"))),
    }
}

/// Relay one exact launcher protocol connection without interpreting verdict
/// contents. The launcher remains the sole Linux executor and the trusted Mac
/// node remains the sole challenge, journal and evidence owner.
pub fn relay_launcher_exchange<HR, HW, LR, LW>(
    phase: ProxyPhase,
    host_input: &mut HR,
    host_output: &mut HW,
    launcher_input: &mut LR,
    launcher_output: &mut LW,
) -> Result<(), ProxyRelayError>
where
    HR: Read,
    HW: Write,
    LR: Read,
    LW: Write,
{
    let first_request = read_bounded_frame(
        host_input,
        PROXY_REQUEST_FRAME_CAP_BYTES,
        "host first request",
    )?;
    write_frame(launcher_output, &first_request, "launcher first request")?;
    let first_response = read_bounded_frame(
        launcher_input,
        PROXY_RESPONSE_FRAME_CAP_BYTES,
        "launcher first response",
    )?;
    write_frame(host_output, &first_response, "host first response")?;

    if phase == ProxyPhase::Execution {
        let request = read_bounded_frame(
            host_input,
            PROXY_REQUEST_FRAME_CAP_BYTES,
            "host execution request",
        )?;
        write_frame(launcher_output, &request, "launcher execution request")?;
        require_eof(host_input, "host execution input")?;
        let report = read_bounded_frame(
            launcher_input,
            PROXY_RESPONSE_FRAME_CAP_BYTES,
            "launcher execution report",
        )?;
        write_frame(host_output, &report, "host execution report")?;
    } else {
        require_eof(host_input, "host qualification input")?;
    }
    require_eof(launcher_input, "launcher response")
}

struct SharedReader<'a, T>(&'a RefCell<&'a mut T>);
struct SharedWriter<'a, T>(&'a RefCell<&'a mut T>);

impl<T: Read> Read for SharedReader<'_, T> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().read(buffer)
    }
}

impl<T: Write> Write for SharedWriter<'_, T> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.borrow_mut().flush()
    }
}

/// Authenticate one proxy open against its exact boot/phase contract, report
/// the kernel-observed root launcher peer, then forward only the phase's fixed
/// length-prefixed frame sequence.
pub fn serve_proxy_connection<H, L>(
    host: &mut H,
    launcher: &mut L,
    launcher_pid: u32,
    launcher_uid: u32,
    launcher_gid: u32,
) -> Result<(), ProxyRelayError>
where
    H: Read + Write,
    L: Read + Write,
{
    let mut open_frame = [0_u8; PROXY_FRAME_BYTES];
    host.read_exact(&mut open_frame)
        .map_err(|error| ProxyRelayError(format!("read proxy open: {error}")))?;
    let hello = decode_proxy_hello(&open_frame)
        .map_err(|error| ProxyRelayError(format!("validate proxy open: {error}")))?;
    let ready = GuestProxyReady::for_hello(&hello, launcher_pid, launcher_uid, launcher_gid)
        .map_err(|error| ProxyRelayError(format!("validate launcher peer: {error}")))?;
    host.write_all(&encode_proxy_ready(&ready))
        .and_then(|()| host.flush())
        .map_err(|error| ProxyRelayError(format!("write proxy ready: {error}")))?;

    let host = RefCell::new(host);
    let launcher = RefCell::new(launcher);
    relay_launcher_exchange(
        hello.phase(),
        &mut SharedReader(&host),
        &mut SharedWriter(&host),
        &mut SharedReader(&launcher),
        &mut SharedWriter(&launcher),
    )
}
