//! N3.1 (ADR-0009) — minimal P2P transport: a `Transport` trait over
//! line-framed JSON/TCP plus the wire-frame enum.
//!
//! The trait is the designed replacement seam (ADR-0009 (a)): a future
//! encrypted transport (Noise/TLS — follow-up ADR, gate (f)) lands behind
//! the same trait without disturbing the frame contract. Plaintext TCP +
//! address allowlist is closed-testnet-only.
//!
//! The wire contract's caps live here as constants and are enforced by the
//! codec itself on both directions (ADR-0009 (c)): oversize input fails
//! *before* buffering past the cap, and an inbound `GetBlocks` is
//! range-validated at decode so no handler has to remember to check.

use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Wire protocol version carried in `Hello` (ADR-0009 (b)) — additive frame
/// evolution bumps this instead of silently breaking the wire.
/// v2: `Hello` gained the required `consensus_rule_version` field
/// (ADR-0009 amendment 2026-07-08, ADR-0014 (b)).
/// v3: useful-work package request/response frames were added (BF.6a).
pub const PROTOCOL_VERSION: u32 = 3;

/// Max encoded frame size, newline included (ADR-0009 (c) — mirrors the MCP
/// stdio frame cap, N0-pre.6). Applies to both directions.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Maximum decoded canonical package payload carried by BF.6a. Base64 at
/// this cap remains below `MAX_FRAME_BYTES`, including JSON metadata.
pub const MAX_PACKAGE_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

/// Max number of blocks one `GetBlocks` may request, inclusive range
/// (ADR-0009 (c)) — longer syncs paginate via repeated requests.
pub const GET_BLOCKS_RANGE_CAP: u64 = 256;

/// Chain-head summary carried in `Hello` (ADR-0009 (b)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadSummary {
    pub height: u64,
    /// Head block hash (64-char lowercase hex).
    pub c: String,
}

/// The five wire frames fixed by ADR-0009 (b). Payloads that re-enter the
/// local admission/replay path (`ShareAnnounce.submission`, `Blocks.blocks`)
/// stay schemaless `Value`s on the wire: ingress re-parses them through the
/// exact local validation path (ADR-0009 (e) — no second validation policy),
/// so the transport layer does not duplicate those schemas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Frame {
    #[serde(rename_all = "camelCase")]
    Hello {
        protocol_version: u32,
        /// Which consensus rule set the sender enforces (ADR-0014 (b);
        /// the constant is owned by boole-core). A mismatch is a typed
        /// disconnect — nodes on different rules must not gossip, or they
        /// silently fork on the same shares.
        consensus_rule_version: u32,
        network_id: String,
        /// Load-bearing for N5.2 (per-network genesis commitment).
        genesis_hash: String,
        head: HeadSummary,
    },
    ShareAnnounce {
        submission: Value,
    },
    BlockAnnounce {
        height: u64,
        c: String,
    },
    GetBlocks {
        from: u64,
        to: u64,
    },
    Blocks {
        blocks: Vec<Value>,
    },
    GetPackage {
        root: String,
    },
    Package {
        root: String,
        #[serde(rename = "canonicalBytesBase64", with = "optional_bytes_base64")]
        canonical_bytes: Option<Vec<u8>>,
    },
}

mod optional_bytes_base64 {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let encoded = bytes
            .as_ref()
            .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes));
        encoded.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = Option::<String>::deserialize(deserializer)?;
        encoded
            .map(|encoded| {
                base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .map_err(serde::de::Error::custom)
            })
            .transpose()
    }
}

impl Frame {
    /// Frame-level contract checks beyond JSON shape. Enforced by the codec
    /// on ingress; send-side runs it before encoding as well.
    pub fn validate(&self) -> Result<(), FrameError> {
        if let Frame::GetBlocks { from, to } = self {
            if to < from || to - from + 1 > GET_BLOCKS_RANGE_CAP {
                return Err(FrameError::RangeTooWide {
                    from: *from,
                    to: *to,
                });
            }
        }
        if let Frame::GetPackage { root } | Frame::Package { root, .. } = self {
            if !is_lowercase_hex32(root) {
                return Err(FrameError::InvalidPackageRoot { root: root.clone() });
            }
        }
        if let Frame::Package {
            canonical_bytes: Some(bytes),
            ..
        } = self
        {
            if bytes.len() > MAX_PACKAGE_PAYLOAD_BYTES {
                return Err(FrameError::PackagePayloadTooLarge { size: bytes.len() });
            }
        }
        Ok(())
    }
}

fn is_lowercase_hex32(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Typed codec/transport errors — ADR-0009 (e) requires cap violations and
/// malformed frames to be distinguishable (typed drop + counted) rather
/// than collapsed into one opaque IO error.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame exceeds MAX_FRAME_BYTES ({MAX_FRAME_BYTES}): saw at least {seen} bytes")]
    FrameTooLarge { seen: usize },
    #[error("malformed frame: {detail}")]
    Malformed { detail: String },
    #[error("GetBlocks range [{from}, {to}] exceeds cap {GET_BLOCKS_RANGE_CAP} or is inverted")]
    RangeTooWide { from: u64, to: u64 },
    #[error("package root must be exactly 32 bytes encoded as lowercase hexadecimal: {root}")]
    InvalidPackageRoot { root: String },
    #[error(
        "decoded package payload is {size} bytes; maximum is {MAX_PACKAGE_PAYLOAD_BYTES} bytes"
    )]
    PackagePayloadTooLarge { size: usize },
    #[error("connection closed before a complete frame")]
    ConnectionClosed,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Transport seam (ADR-0009 (a)): `connect`/`accept` + framed send/recv.
/// `accept` surfaces the peer address because the configured peer set
/// doubles as the inbound allowlist (ADR-0009 (d)) — the caller drops
/// non-allowlisted addresses at accept.
pub trait Transport {
    type Conn;
    type Listener;

    fn bind(&self, addr: &str) -> Result<Self::Listener, FrameError>;
    fn local_addr(&self, listener: &Self::Listener) -> Result<SocketAddr, FrameError>;
    fn accept(&self, listener: &mut Self::Listener)
        -> Result<(Self::Conn, SocketAddr), FrameError>;
    fn connect(&self, addr: &SocketAddr) -> Result<Self::Conn, FrameError>;
    fn send_frame(&self, conn: &mut Self::Conn, frame: &Frame) -> Result<(), FrameError>;
    fn recv_frame(&self, conn: &mut Self::Conn) -> Result<Frame, FrameError>;
}

/// A live TCP connection: buffered reader/writer over one stream.
pub struct TcpConn {
    reader: BufReader<TcpStream>,
    writer: BufWriter<TcpStream>,
}

/// Line-framed JSON over plain TCP (ADR-0009 (a)) — closed-testnet-only
/// posture per gate (f).
#[derive(Debug, Default, Clone, Copy)]
pub struct TcpTransport;

impl TcpTransport {
    pub fn new() -> Self {
        Self
    }

    /// Wrap an already-connected stream in the frame codec. Public so a
    /// caller that must own socket options (nonblocking accept loops, read
    /// and write timeouts — N3.2 gossip ingress/egress) can accept/connect
    /// itself and still speak the exact wire contract.
    pub fn conn_from_stream(stream: TcpStream) -> Result<TcpConn, FrameError> {
        let write_half = stream.try_clone()?;
        Ok(TcpConn {
            reader: BufReader::new(stream),
            writer: BufWriter::new(write_half),
        })
    }
}

impl Transport for TcpTransport {
    type Conn = TcpConn;
    type Listener = TcpListener;

    fn bind(&self, addr: &str) -> Result<Self::Listener, FrameError> {
        Ok(TcpListener::bind(addr)?)
    }

    fn local_addr(&self, listener: &Self::Listener) -> Result<SocketAddr, FrameError> {
        Ok(listener.local_addr()?)
    }

    fn accept(
        &self,
        listener: &mut Self::Listener,
    ) -> Result<(Self::Conn, SocketAddr), FrameError> {
        let (stream, peer) = listener.accept()?;
        Ok((Self::conn_from_stream(stream)?, peer))
    }

    fn connect(&self, addr: &SocketAddr) -> Result<Self::Conn, FrameError> {
        let stream = TcpStream::connect(addr)?;
        Self::conn_from_stream(stream)
    }

    fn send_frame(&self, conn: &mut Self::Conn, frame: &Frame) -> Result<(), FrameError> {
        frame.validate()?;
        let mut encoded = serde_json::to_string(frame).map_err(|err| FrameError::Malformed {
            detail: format!("encode: {err}"),
        })?;
        encoded.push('\n');
        if encoded.len() > MAX_FRAME_BYTES {
            return Err(FrameError::FrameTooLarge {
                seen: encoded.len(),
            });
        }
        conn.writer.write_all(encoded.as_bytes())?;
        conn.writer.flush()?;
        Ok(())
    }

    fn recv_frame(&self, conn: &mut Self::Conn) -> Result<Frame, FrameError> {
        let line = read_line_capped(&mut conn.reader, MAX_FRAME_BYTES)?;
        let frame: Frame = serde_json::from_slice(&line).map_err(|err| FrameError::Malformed {
            detail: err.to_string(),
        })?;
        frame.validate()?;
        Ok(frame)
    }
}

/// Read one newline-terminated line, failing with `FrameTooLarge` as soon
/// as `cap` bytes have accumulated without a newline — the buffer never
/// grows past the cap, which is the ADR-0009 (c) DoS bound.
fn read_line_capped<R: Read>(reader: &mut BufReader<R>, cap: usize) -> Result<Vec<u8>, FrameError> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if line.is_empty() {
                Err(FrameError::ConnectionClosed)
            } else {
                Err(FrameError::Malformed {
                    detail: "connection closed mid-frame".to_string(),
                })
            };
        }
        if let Some(newline_at) = available.iter().position(|&b| b == b'\n') {
            if line.len() + newline_at + 1 > cap {
                return Err(FrameError::FrameTooLarge {
                    seen: line.len() + newline_at + 1,
                });
            }
            line.extend_from_slice(&available[..newline_at]);
            reader.consume(newline_at + 1);
            return Ok(line);
        }
        let taken = available.len();
        // `+ 1` — the eventual newline counts toward the cap (send-side
        // symmetry: MAX_FRAME_BYTES includes the terminator).
        if line.len() + taken + 1 > cap {
            return Err(FrameError::FrameTooLarge {
                seen: line.len() + taken + 1,
            });
        }
        line.extend_from_slice(available);
        reader.consume(taken);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_encodes_with_camel_case_tag_and_fields() {
        let frame = Frame::Hello {
            protocol_version: PROTOCOL_VERSION,
            consensus_rule_version: 1,
            network_id: "net".to_string(),
            genesis_hash: "00".repeat(32),
            head: HeadSummary {
                height: 1,
                c: "ab".repeat(32),
            },
        };
        let encoded = serde_json::to_value(&frame).expect("encode");
        assert_eq!(encoded["type"], "hello");
        assert_eq!(encoded["protocolVersion"], 3);
        assert_eq!(encoded["consensusRuleVersion"], 1);
        assert_eq!(encoded["networkId"], "net");
        assert!(encoded["genesisHash"].is_string());
        assert_eq!(encoded["head"]["height"], 1);
    }

    #[test]
    fn read_line_capped_stops_at_exact_cap() {
        let data = [b'x'; 10];
        let mut reader = BufReader::new(&data[..]);
        let err = read_line_capped(&mut reader, 10).expect_err("no newline within cap");
        assert!(matches!(err, FrameError::FrameTooLarge { .. }));

        let mut ok = b"12345678\n".to_vec();
        ok.extend_from_slice(b"rest");
        let mut reader = BufReader::new(&ok[..]);
        let line = read_line_capped(&mut reader, 10).expect("line within cap");
        assert_eq!(line, b"12345678");
    }
}
