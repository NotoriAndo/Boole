use std::fmt;
use std::io::{Read, Write};
use std::sync::Mutex;

use sha2::{Digest, Sha256};

const CONTROLLER_MAGIC: [u8; 8] = *b"BOOLE4C1";
const CONTROLLER_VERSION: u8 = 1;
const CONTROLLER_HEADER_BYTES: usize = 96;
pub(crate) const CONTROLLER_FRAME_CAP_BYTES: usize = 524_288;
const CONTROLLER_FRAME_COUNT_CAP: usize = 3;
const CONTROLLER_PAYLOAD_CAP_BYTES: usize =
    CONTROLLER_FRAME_COUNT_CAP * (CONTROLLER_FRAME_CAP_BYTES + 4);
const CONTROLLER_CONTRACT_DIGEST: [u8; 32] = [
    0x98, 0x09, 0x5a, 0xbd, 0xe0, 0xcb, 0x32, 0xcb, 0x5f, 0xb2, 0x7e, 0xde, 0xaf, 0x5b, 0xc6, 0xc6,
    0x7f, 0x3d, 0xf7, 0x96, 0xad, 0x3c, 0xda, 0x07, 0xb1, 0x6f, 0x8b, 0x44, 0x84, 0xb9, 0xb7, 0x13,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControllerCommand {
    Qualification = 1,
    Execution = 2,
    Shutdown = 3,
}

impl ControllerCommand {
    fn response_kind(self) -> u8 {
        self as u8 | 0x80
    }

    fn request_frame_count(self) -> usize {
        match self {
            Self::Qualification => 1,
            Self::Execution => 2,
            Self::Shutdown => 0,
        }
    }

    fn response_frame_count(self) -> usize {
        match self {
            Self::Qualification => 2,
            Self::Execution => 3,
            Self::Shutdown => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ControllerLauncherPeer {
    pub(crate) pid: u32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
}

impl ControllerLauncherPeer {
    fn require_root(self) -> Result<Self, ControllerError> {
        if self.pid == 0 || self.uid != 0 || self.gid != 0 {
            return Err(ControllerError(
                "controller launcher peer is not root".into(),
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ControllerQualificationOutput {
    pub(crate) launcher_peer: ControllerLauncherPeer,
    pub(crate) proxy_ready_frame: Vec<u8>,
    pub(crate) launcher_ready_frame: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ControllerExecutionOutput {
    pub(crate) launcher_peer: ControllerLauncherPeer,
    pub(crate) proxy_ready_frame: Vec<u8>,
    pub(crate) launcher_ready_frame: Vec<u8>,
    pub(crate) launcher_report_frame: Vec<u8>,
}

#[derive(Debug)]
pub struct ControllerError(String);

impl fmt::Display for ControllerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ControllerError {}

struct ControllerIo<R, W> {
    reader: R,
    writer: W,
    stopped: bool,
}

/// Serialized node-side owner of one persistent Mac host-controller stream.
///
/// Construct this only from the private stdin/stdout pipes of the verified
/// `host-controller` artifact. It is transport plumbing and owns no verdict,
/// challenge, journal, reward, consensus or activation authority.
pub struct Mac4ControllerClient<R, W> {
    io: Mutex<ControllerIo<R, W>>,
}

impl<R, W> fmt::Debug for Mac4ControllerClient<R, W> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Mac4ControllerClient")
            .finish_non_exhaustive()
    }
}

impl<R: Read, W: Write> Mac4ControllerClient<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            io: Mutex::new(ControllerIo {
                reader,
                writer,
                stopped: false,
            }),
        }
    }

    pub(crate) fn qualify(
        &self,
        launcher_hello_frame: &[u8],
    ) -> Result<ControllerQualificationOutput, ControllerError> {
        let response = self.transact(ControllerCommand::Qualification, &[launcher_hello_frame])?;
        let [proxy_ready_frame, launcher_ready_frame]: [Vec<u8>; 2] = response
            .frames
            .try_into()
            .map_err(|_| ControllerError("qualification frame count differs".into()))?;
        Ok(ControllerQualificationOutput {
            launcher_peer: response
                .launcher_peer
                .ok_or_else(|| ControllerError("qualification launcher peer absent".into()))?,
            proxy_ready_frame,
            launcher_ready_frame,
        })
    }

    pub(crate) fn execute(
        &self,
        launcher_hello_frame: &[u8],
        launcher_request_frame: &[u8],
    ) -> Result<ControllerExecutionOutput, ControllerError> {
        let response = self.transact(
            ControllerCommand::Execution,
            &[launcher_hello_frame, launcher_request_frame],
        )?;
        let [proxy_ready_frame, launcher_ready_frame, launcher_report_frame]: [Vec<u8>; 3] =
            response
                .frames
                .try_into()
                .map_err(|_| ControllerError("execution frame count differs".into()))?;
        Ok(ControllerExecutionOutput {
            launcher_peer: response
                .launcher_peer
                .ok_or_else(|| ControllerError("execution launcher peer absent".into()))?,
            proxy_ready_frame,
            launcher_ready_frame,
            launcher_report_frame,
        })
    }

    pub fn shutdown(&self) -> Result<(), ControllerError> {
        self.transact(ControllerCommand::Shutdown, &[])?;
        Ok(())
    }

    fn transact(
        &self,
        command: ControllerCommand,
        frames: &[&[u8]],
    ) -> Result<DecodedEnvelope, ControllerError> {
        let mut io = self
            .io
            .lock()
            .map_err(|_| ControllerError("controller I/O lock is poisoned".into()))?;
        if io.stopped {
            return Err(ControllerError("controller is already stopped".into()));
        }
        if frames.len() != command.request_frame_count() {
            return Err(ControllerError(
                "controller request frame count differs".into(),
            ));
        }
        let id = request_id(command, frames);
        let request = encode_envelope(command as u8, id, None, frames)?;
        io.writer
            .write_all(&request)
            .and_then(|()| io.writer.flush())
            .map_err(|error| ControllerError(format!("write controller request: {error}")))?;
        let response = read_envelope(&mut io.reader)?;
        if response.kind != command.response_kind() {
            return Err(ControllerError("controller response kind differs".into()));
        }
        if response.request_id != id {
            return Err(ControllerError(
                "controller response request binding differs".into(),
            ));
        }
        if response.frames.len() != command.response_frame_count() {
            return Err(ControllerError(
                "controller response frame count differs".into(),
            ));
        }
        match command {
            ControllerCommand::Qualification | ControllerCommand::Execution => {
                response
                    .launcher_peer
                    .ok_or_else(|| ControllerError("controller launcher peer absent".into()))?
                    .require_root()?;
            }
            ControllerCommand::Shutdown => {
                if response.launcher_peer.is_some() {
                    return Err(ControllerError(
                        "shutdown response forged launcher peer".into(),
                    ));
                }
                io.stopped = true;
            }
        }
        Ok(response)
    }

    #[cfg(test)]
    fn into_inner(self) -> Result<(R, W), ControllerError> {
        let inner = self
            .io
            .into_inner()
            .map_err(|_| ControllerError("controller I/O lock is poisoned".into()))?;
        Ok((inner.reader, inner.writer))
    }
}

#[derive(Debug)]
struct DecodedEnvelope {
    kind: u8,
    request_id: [u8; 32],
    launcher_peer: Option<ControllerLauncherPeer>,
    frames: Vec<Vec<u8>>,
}

pub(crate) fn request_id(command: ControllerCommand, frames: &[&[u8]]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update([command as u8]);
    for frame in frames {
        digest.update((frame.len() as u32).to_be_bytes());
        digest.update(frame);
    }
    digest.finalize().into()
}

fn encode_envelope(
    kind: u8,
    request_id: [u8; 32],
    launcher_peer: Option<ControllerLauncherPeer>,
    frames: &[&[u8]],
) -> Result<Vec<u8>, ControllerError> {
    if frames.len() > CONTROLLER_FRAME_COUNT_CAP {
        return Err(ControllerError("controller frame count exceeds cap".into()));
    }
    let mut payload = Vec::new();
    for frame in frames {
        if frame.len() > CONTROLLER_FRAME_CAP_BYTES {
            return Err(ControllerError("controller frame exceeds cap".into()));
        }
        payload.extend_from_slice(&(frame.len() as u32).to_be_bytes());
        payload.extend_from_slice(frame);
    }
    if payload.len() > CONTROLLER_PAYLOAD_CAP_BYTES {
        return Err(ControllerError("controller payload exceeds cap".into()));
    }
    let mut output = vec![0_u8; CONTROLLER_HEADER_BYTES];
    output[..8].copy_from_slice(&CONTROLLER_MAGIC);
    output[8] = CONTROLLER_VERSION;
    output[9] = kind;
    output[10..12].copy_from_slice(&(frames.len() as u16).to_be_bytes());
    output[12..16].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    output[16..48].copy_from_slice(&request_id);
    output[48..80].copy_from_slice(&CONTROLLER_CONTRACT_DIGEST);
    if let Some(peer) = launcher_peer {
        output[80..84].copy_from_slice(&peer.pid.to_be_bytes());
        output[84..88].copy_from_slice(&peer.uid.to_be_bytes());
        output[88..92].copy_from_slice(&peer.gid.to_be_bytes());
    }
    output.extend_from_slice(&payload);
    Ok(output)
}

fn read_envelope(reader: &mut impl Read) -> Result<DecodedEnvelope, ControllerError> {
    let mut header = [0_u8; CONTROLLER_HEADER_BYTES];
    reader
        .read_exact(&mut header)
        .map_err(|error| ControllerError(format!("read controller header: {error}")))?;
    if header[..8] != CONTROLLER_MAGIC
        || header[8] != CONTROLLER_VERSION
        || header[48..80] != CONTROLLER_CONTRACT_DIGEST
        || header[92..96] != [0, 0, 0, 0]
    {
        return Err(ControllerError("controller header identity differs".into()));
    }
    let frame_count = u16::from_be_bytes([header[10], header[11]]) as usize;
    let payload_len =
        u32::from_be_bytes(header[12..16].try_into().expect("fixed header range")) as usize;
    if frame_count > CONTROLLER_FRAME_COUNT_CAP || payload_len > CONTROLLER_PAYLOAD_CAP_BYTES {
        return Err(ControllerError("controller response exceeds cap".into()));
    }
    let mut request_id = [0_u8; 32];
    request_id.copy_from_slice(&header[16..48]);
    let pid = u32::from_be_bytes(header[80..84].try_into().expect("fixed peer range"));
    let uid = u32::from_be_bytes(header[84..88].try_into().expect("fixed peer range"));
    let gid = u32::from_be_bytes(header[88..92].try_into().expect("fixed peer range"));
    let launcher_peer = if (pid, uid, gid) == (0, 0, 0) {
        None
    } else {
        Some(ControllerLauncherPeer { pid, uid, gid })
    };
    let mut payload = vec![0_u8; payload_len];
    reader
        .read_exact(&mut payload)
        .map_err(|error| ControllerError(format!("read controller payload: {error}")))?;
    let mut offset = 0;
    let mut frames = Vec::with_capacity(frame_count);
    for _ in 0..frame_count {
        if payload.len().saturating_sub(offset) < 4 {
            return Err(ControllerError(
                "controller frame header is truncated".into(),
            ));
        }
        let length = u32::from_be_bytes(
            payload[offset..offset + 4]
                .try_into()
                .expect("checked frame header"),
        ) as usize;
        offset += 4;
        if length > CONTROLLER_FRAME_CAP_BYTES || payload.len().saturating_sub(offset) < length {
            return Err(ControllerError(
                "controller frame is truncated or oversized".into(),
            ));
        }
        frames.push(payload[offset..offset + length].to_vec());
        offset += length;
    }
    if offset != payload.len() {
        return Err(ControllerError(
            "controller payload has trailing bytes".into(),
        ));
    }
    Ok(DecodedEnvelope {
        kind: header[9],
        request_id,
        launcher_peer,
        frames,
    })
}

#[cfg(test)]
pub(crate) fn encode_response_for_test(
    command: ControllerCommand,
    request_id: [u8; 32],
    launcher_peer: Option<ControllerLauncherPeer>,
    frames: &[&[u8]],
) -> Vec<u8> {
    encode_envelope(command.response_kind(), request_id, launcher_peer, frames)
        .expect("test response")
}

#[cfg(test)]
fn encode_request_for_test(
    command: ControllerCommand,
    frames: &[&[u8]],
) -> Result<Vec<u8>, ControllerError> {
    encode_envelope(command as u8, request_id(command, frames), None, frames)
}

#[cfg(test)]
fn decode_request_kinds(mut bytes: &[u8]) -> Vec<u8> {
    let mut kinds = Vec::new();
    while !bytes.is_empty() {
        let before = bytes.len();
        let decoded = read_envelope(&mut bytes).expect("request envelope");
        kinds.push(decoded.kind);
        assert!(bytes.len() < before);
    }
    kinds
}

#[cfg(test)]
mod tests {
    use super::{
        encode_response_for_test, ControllerCommand, ControllerLauncherPeer, Mac4ControllerClient,
    };
    use std::io::{Cursor, Write};

    #[test]
    fn persistent_controller_qualifies_once_and_serves_multiple_executions() {
        let peer = ControllerLauncherPeer {
            pid: 4242,
            uid: 0,
            gid: 0,
        };
        let qualification_ready = b"qualification-ready".to_vec();
        let execution_ready_one = b"execution-ready-one".to_vec();
        let execution_report_one = b"execution-report-one".to_vec();
        let execution_ready_two = b"execution-ready-two".to_vec();
        let execution_report_two = b"execution-report-two".to_vec();
        let qualification_id = super::request_id(ControllerCommand::Qualification, &[b"q"]);
        let execution_one_id = super::request_id(ControllerCommand::Execution, &[b"h1", b"r1"]);
        let execution_two_id = super::request_id(ControllerCommand::Execution, &[b"h2", b"r2"]);
        let shutdown_id = super::request_id(ControllerCommand::Shutdown, &[]);
        let input = [
            encode_response_for_test(
                ControllerCommand::Qualification,
                qualification_id,
                Some(peer),
                &[b"proxy-q".as_slice(), qualification_ready.as_slice()],
            ),
            encode_response_for_test(
                ControllerCommand::Execution,
                execution_one_id,
                Some(peer),
                &[
                    b"proxy-e1".as_slice(),
                    execution_ready_one.as_slice(),
                    execution_report_one.as_slice(),
                ],
            ),
            encode_response_for_test(
                ControllerCommand::Execution,
                execution_two_id,
                Some(peer),
                &[
                    b"proxy-e2".as_slice(),
                    execution_ready_two.as_slice(),
                    execution_report_two.as_slice(),
                ],
            ),
            encode_response_for_test(ControllerCommand::Shutdown, shutdown_id, None, &[]),
        ]
        .concat();
        let reader = Cursor::new(input);
        let writer = Vec::<u8>::new();
        let controller = Mac4ControllerClient::new(reader, writer);

        let qualification = controller.qualify(b"q").expect("qualification");
        assert_eq!(qualification.launcher_peer, peer);
        assert_eq!(qualification.launcher_ready_frame, qualification_ready);
        let first = controller.execute(b"h1", b"r1").expect("first execution");
        assert_eq!(first.launcher_peer, peer);
        assert_eq!(first.launcher_ready_frame, execution_ready_one);
        assert_eq!(first.launcher_report_frame, execution_report_one);
        let second = controller.execute(b"h2", b"r2").expect("second execution");
        assert_eq!(second.launcher_peer, peer);
        assert_eq!(second.launcher_ready_frame, execution_ready_two);
        assert_eq!(second.launcher_report_frame, execution_report_two);
        controller.shutdown().expect("shutdown");

        let (_, written) = controller.into_inner().expect("sole controller owner");
        assert_eq!(super::decode_request_kinds(&written), vec![1, 2, 2, 3]);
    }

    #[test]
    fn controller_rejects_wrong_request_binding_or_launcher_peer() {
        let peer = ControllerLauncherPeer {
            pid: 4242,
            uid: 0,
            gid: 0,
        };
        let wrong_id = [0x55; 32];
        let input = encode_response_for_test(
            ControllerCommand::Execution,
            wrong_id,
            Some(peer),
            &[
                b"proxy".as_slice(),
                b"ready".as_slice(),
                b"report".as_slice(),
            ],
        );
        let controller = Mac4ControllerClient::new(Cursor::new(input), Vec::new());
        assert!(controller.execute(b"hello", b"request").is_err());

        let zero_peer = ControllerLauncherPeer {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let request_id = super::request_id(
            ControllerCommand::Execution,
            &[b"hello".as_slice(), b"request".as_slice()],
        );
        let input = encode_response_for_test(
            ControllerCommand::Execution,
            request_id,
            Some(zero_peer),
            &[
                b"proxy".as_slice(),
                b"ready".as_slice(),
                b"report".as_slice(),
            ],
        );
        let controller = Mac4ControllerClient::new(Cursor::new(input), Vec::new());
        assert!(controller.execute(b"hello", b"request").is_err());
    }

    #[test]
    fn controller_rejects_oversized_or_trailing_response_payloads() {
        let peer = ControllerLauncherPeer {
            pid: 4242,
            uid: 0,
            gid: 0,
        };
        let request_id = super::request_id(
            ControllerCommand::Execution,
            &[b"hello".as_slice(), b"request".as_slice()],
        );
        let mut response = encode_response_for_test(
            ControllerCommand::Execution,
            request_id,
            Some(peer),
            &[
                b"proxy".as_slice(),
                b"ready".as_slice(),
                b"report".as_slice(),
            ],
        );
        response.write_all(b"trailing").unwrap();
        let controller = Mac4ControllerClient::new(Cursor::new(response), Vec::new());
        controller
            .execute(b"hello", b"request")
            .expect("first exact response");
        assert!(
            controller.shutdown().is_err(),
            "trailing bytes cannot form a response"
        );

        let oversized = vec![0_u8; super::CONTROLLER_FRAME_CAP_BYTES + 1];
        assert!(
            super::encode_request_for_test(ControllerCommand::Execution, &[&oversized]).is_err()
        );
    }
}
