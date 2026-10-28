//! Host contract for a trusted controller. This is private local evidence, not
//! D's future native termination/status grammar or a proof-verdict authority.
use crate::{
    codec::digest,
    real_artifact::local::{HeldArtifact, Profile, Role},
    real_deadline::Deadline,
    real_descriptor::{ascii, Descriptor},
    real_reply::Context,
};
use anyhow::{ensure, Result};
use boole_derivation::m2::Hash;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Preflight,
    Decode,
    Front,
    Kernel,
    Recheck,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tool {
    Adapter,
    Decoder,
    Front,
    Comparator,
    Nanoda,
    LeanReplay,
}
impl Tool {
    fn allowed(self, phase: Phase) -> bool {
        matches!(
            (phase, self),
            (Phase::Preflight, Tool::Adapter)
                | (Phase::Decode, Tool::Decoder)
                | (Phase::Front, Tool::Front)
                | (
                    Phase::Kernel,
                    Tool::Adapter | Tool::Comparator | Tool::Nanoda | Tool::LeanReplay
                )
                | (Phase::Recheck, Tool::Adapter)
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Limits {
    pub memory_mib: u64,
    pub wall_ms: u64,
    pub output_bytes: usize,
}
impl Limits {
    fn validate(self) -> Result<()> {
        ensure!(
            self.memory_mib > 0
                && self.memory_mib <= 32768
                && self.wall_ms > 0
                && self.wall_ms <= 3600000
                && self.output_bytes > 0
                && self.output_bytes <= 65536,
            "controller envelope outside local protocol bounds"
        );
        Ok(())
    }
}
/// Addresses are supplied by the authenticated package/descriptor, never learned
/// from the local manifest. Local integrity alone does not certify their release.
pub struct Input {
    artifact: HeldArtifact,
}
impl Input {
    pub fn new(
        artifact: HeldArtifact,
        expected: Hash,
        role: Role,
        profile: Profile,
        deadline: Deadline,
    ) -> Result<Self> {
        let b = artifact.binding();
        ensure!(
            b.sha256 == expected && b.role == role && b.profile == profile,
            "controller artifact binding"
        );
        artifact.verify(deadline)?;
        Ok(Self { artifact })
    }
    pub fn artifact(&self) -> &HeldArtifact {
        &self.artifact
    }
}
pub struct Invocation<'a> {
    pub descriptor: &'a [u8],
    pub context: &'a Context,
    pub epoch: u32,
    pub decoder: Hash,
    pub transport: Hash,
    pub profile: Profile,
}
/// Trusted release/reconstruction seam. Implementations must obtain expected
/// values independently, never echo the invocation being checked. This build
/// ships no production provider; the default constructor refuses unknown pins.
pub trait InvocationPins {
    fn decoder(&self, descriptor: &Descriptor, profile: Profile) -> Option<Hash>;
    fn transport(&self, context: &Context) -> Option<Hash>;
}
pub struct UnknownPins;
impl InvocationPins for UnknownPins {
    fn decoder(&self, _descriptor: &Descriptor, _profile: Profile) -> Option<Hash> {
        None
    }
    fn transport(&self, _context: &Context) -> Option<Hash> {
        None
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinError {
    ProfileUnavailable,
    UnknownDecoder,
    DecoderMismatch,
    UnknownTransport,
    TransportMismatch,
}
impl std::fmt::Display for PinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "controller pin unavailable or mismatched: {self:?}")
    }
}
impl std::error::Error for PinError {}
pub struct Request {
    phase: Phase,
    tool: Tool,
    limits: Limits,
    inputs: Vec<Input>,
    binding: Hash,
    input_binding: Hash,
}
impl Request {
    pub fn new(
        phase: Phase,
        tool: Tool,
        limits: Limits,
        invocation: Invocation<'_>,
        inputs: Vec<Input>,
        deadline: Deadline,
    ) -> Result<Self> {
        Self::new_with_pins(
            phase,
            tool,
            limits,
            invocation,
            inputs,
            deadline,
            &UnknownPins,
        )
    }
    pub fn new_with_pins(
        phase: Phase,
        tool: Tool,
        limits: Limits,
        invocation: Invocation<'_>,
        inputs: Vec<Input>,
        deadline: Deadline,
        pins: &dyn InvocationPins,
    ) -> Result<Self> {
        deadline.check()?;
        limits.validate()?;
        ensure!(tool.allowed(phase), "controller phase/tool mismatch");
        let d = Descriptor::decode(invocation.descriptor)?;
        invocation.context.validate()?;
        let profile_key = match invocation.profile {
            Profile::DarwinArm64 => "Darwin-arm64",
            Profile::LinuxAarch64 => "Linux-aarch64",
        };
        d.profiles
            .get(profile_key)
            .ok_or(PinError::ProfileUnavailable)?;
        let decoder = pins
            .decoder(&d, invocation.profile)
            .filter(|p| *p != [0; 32])
            .ok_or(PinError::UnknownDecoder)?;
        if invocation.decoder != decoder {
            return Err(PinError::DecoderMismatch.into());
        }
        let transport = pins
            .transport(invocation.context)
            .filter(|p| *p != [0; 32])
            .ok_or(PinError::UnknownTransport)?;
        if invocation.transport != transport {
            return Err(PinError::TransportMismatch.into());
        }
        ensure!(
            invocation.decoder != [0; 32] && invocation.transport != [0; 32],
            "controller decoder/transport binding"
        );
        ensure!(inputs.len() <= 256, "controller input handle cap");
        for i in &inputs {
            ensure!(
                i.artifact.binding().profile == invocation.profile,
                "controller input profile mismatch"
            );
            i.artifact.verify(deadline)?;
        }
        let mut nonce = [0; 32];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce)
            .map_err(|_| anyhow::anyhow!("controller invocation entropy"))?;
        let addresses: Vec<_> = inputs.iter().map(|i| i.artifact.binding()).collect();
        let bytes = ascii(&(
            phase,
            tool,
            limits,
            digest(&ascii(invocation.context)?),
            d.digest(),
            digest(invocation.descriptor),
            invocation.epoch,
            invocation.decoder,
            invocation.transport,
            invocation.profile,
            addresses,
        ))?;
        let input_binding =
            digest(&[b"BOOLE-M3/CONTROLLER-INPUT/v1\0".as_slice(), &bytes].concat());
        let binding = digest(
            &[
                b"BOOLE-M3/CONTROLLER-REQUEST/v1\0".as_slice(),
                input_binding.as_slice(),
                nonce.as_slice(),
            ]
            .concat(),
        );
        Ok(Self {
            phase,
            tool,
            limits,
            inputs,
            binding,
            input_binding,
        })
    }
    pub fn binding(&self) -> Hash {
        self.binding
    }
    pub fn input_binding(&self) -> Hash {
        self.input_binding
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn tool(&self) -> Tool {
        self.tool
    }
    pub fn limits(&self) -> Limits {
        self.limits
    }
    pub fn inputs(&self) -> &[Input] {
        &self.inputs
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PhysicalStop {
    Wall,
    Memory,
    Tasks,
    Capture,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unavailable {
    Backend,
    Tool,
    Provisioning,
    Protocol,
    Read,
    Write,
    Cleanup,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MissingProducer {
    Depth,
    KernelPanic,
    CapOrigin,
    PreKernelCapOrigin,
    StructuredSelection,
    Memory,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "kebab-case")]
pub enum Observation {
    Finished { exit: i32 },
    Physical { stop: PhysicalStop },
    Unavailable { cause: Unavailable },
    NativeClaim { cause: MissingProducer },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeferClass {
    T,
    DNode,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    Completed,
    Defer(DeferClass),
}
impl Observation {
    pub fn disposition(self) -> Disposition {
        match self {
            Self::Finished { exit: 0 } => Disposition::Completed,
            Self::Physical { .. } => Disposition::Defer(DeferClass::T),
            _ => Disposition::Defer(DeferClass::DNode),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    request_sha256: String,
    phase: Phase,
    tool: Tool,
    observation: Observation,
}
pub struct Evidence {
    observation: Observation,
    phase: Phase,
}
impl Evidence {
    /// Caller owns a private bounded channel. No exit/stderr classifier exists.
    pub fn decode(raw: &[u8], request: &Request) -> std::result::Result<Self, Fault> {
        let protocol = || Fault {
            phase: request.phase,
            observation: Observation::Unavailable {
                cause: Unavailable::Protocol,
            },
        };
        if raw.len() > 16384 {
            return Err(protocol());
        }
        let record: Record = serde_json::from_slice(raw).map_err(|_| protocol())?;
        if record.version != 1
            || ascii(&record).map_err(|_| protocol())? != raw
            || record.request_sha256
                != boole_derivation::attributes::hex_string(&request.binding)[2..]
            || record.phase != request.phase
            || record.tool != request.tool
        {
            return Err(protocol());
        }
        Ok(Self {
            observation: record.observation,
            phase: record.phase,
        })
    }
    pub fn disposition(&self) -> Disposition {
        self.observation.disposition()
    }
    pub fn fault(&self) -> Option<Fault> {
        if self.disposition() == Disposition::Completed {
            None
        } else {
            Some(Fault {
                phase: self.phase,
                observation: self.observation,
            })
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Fault {
    phase: Phase,
    observation: Observation,
}
impl Fault {
    pub fn physical(phase: Phase, stop: PhysicalStop) -> Self {
        Self {
            phase,
            observation: Observation::Physical { stop },
        }
    }
    pub fn unavailable(phase: Phase, cause: Unavailable) -> Self {
        Self {
            phase,
            observation: Observation::Unavailable { cause },
        }
    }
    pub fn phase(self) -> Phase {
        self.phase
    }
    pub fn class(self) -> DeferClass {
        match self.observation.disposition() {
            Disposition::Defer(c) => c,
            Disposition::Completed => DeferClass::DNode,
        }
    }
}
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "controller {:?}: {:?}", self.phase, self.observation)
    }
}
impl std::error::Error for Fault {}
/// A successful OS boundary still requires the existing strict Reply validator.
/// It is not an ACCEPT, a native termination receipt, or a production attestation.
pub struct Execution {
    evidence: Evidence,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}
impl Execution {
    pub fn from_channels(
        request: &Request,
        status: &[u8],
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    ) -> std::result::Result<Self, Fault> {
        let evidence = Evidence::decode(status, request)?;
        if let Some(fault) = evidence.fault() {
            return Err(fault);
        }
        if stdout.len() > request.limits.output_bytes || stderr.len() > request.limits.output_bytes
        {
            return Err(Fault::physical(request.phase, PhysicalStop::Capture));
        }
        Ok(Self {
            evidence,
            stdout,
            stderr,
        })
    }
    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }
    pub fn evidence(&self) -> &Evidence {
        &self.evidence
    }
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }
}
