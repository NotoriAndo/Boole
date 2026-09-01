//! Fixed-size MAC.4 host/guest liveness and boot-binding handshake.
//!
//! Authentication is deliberately split across layers. The kernel-reported
//! AF_VSOCK peer CID authenticates the Mac host to the guest. On the Mac, the
//! `VZVirtioSocketDevice` used to connect is owned by the exact VM whose boot
//! files were verified before start. This frame adds freshness, protocol
//! versioning and an exact boot-tuple binding without inventing a second
//! challenge, replay, journal or verdict authority inside the guest.

use std::fmt;

pub const VSOCK_PORT: u32 = 4050;
pub const HOST_CID: u32 = 2;
pub const FRAME_BYTES: usize = 108;
pub const CONTRACT_SHA256_HEX: &str =
    "4f2ec110d72f628207ac383668daff7bda6b568449fd315d8376aeb20ae08bbd";

const MAGIC: [u8; 8] = *b"BOOLE4V1";
const HELLO_KIND: u8 = 1;
const READY_KIND: u8 = 2;
const PROTOCOL_BINDING: [u8; 32] = [
    0x4f, 0x2e, 0xc1, 0x10, 0xd7, 0x2f, 0x62, 0x82, 0x07, 0xac, 0x38, 0x36, 0x68, 0xda, 0xff, 0x7b,
    0xda, 0x6b, 0x56, 0x84, 0x49, 0xfd, 0x31, 0x5d, 0x83, 0x76, 0xae, 0xb2, 0x0a, 0xe0, 0x8b, 0xbd,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameError(&'static str);

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for FrameError {}

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
