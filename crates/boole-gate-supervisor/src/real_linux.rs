//! Explicit production refusal seams. No Linux controller is enabled by this build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prerequisite {
    ControllerBackend,
    ProductionContract,
    NativeCauses,
    MemoryAccounting,
    ArtifactRelease,
    RuntimeClosure,
    SandboxCapabilities,
}
pub const PENDING: [Prerequisite; 7] = [
    Prerequisite::ControllerBackend,
    Prerequisite::ProductionContract,
    Prerequisite::NativeCauses,
    Prerequisite::MemoryAccounting,
    Prerequisite::ArtifactRelease,
    Prerequisite::RuntimeClosure,
    Prerequisite::SandboxCapabilities,
];
#[derive(Debug)]
pub struct StartupRefusal {
    pub profile: String,
    pub pending: Vec<Prerequisite>,
}
impl std::fmt::Display for StartupRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.profile == "unsupported" {
            return f.write_str("S16: unsupported real controller host profile");
        }
        write!(
            f,
            "S16: Linux controller prerequisites unavailable: {:?}",
            self.pending
        )
    }
}
impl std::error::Error for StartupRefusal {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dispatch {
    MacDevelopment,
}
/// Profile selection is not a capability grant. There is deliberately no Linux
/// success token and no caller-supplied readiness boolean in this generation.
pub fn select(profile: &str) -> Result<Dispatch, StartupRefusal> {
    match profile {
        "darwin-aarch64" => Ok(Dispatch::MacDevelopment),
        "linux-aarch64" => Err(StartupRefusal {
            profile: "Linux-aarch64".into(),
            pending: PENDING.to_vec(),
        }),
        _ => Err(StartupRefusal {
            profile: "unsupported".into(),
            pending: vec![],
        }),
    }
}

/// Typed seam only. No command, fallback sandbox or capability probe is started
/// before the production backend and release prerequisites are implemented.
pub fn execute(
    request: &crate::real_controller::Request,
) -> Result<crate::real_controller::Execution, crate::real_controller::Fault> {
    Err(crate::real_controller::Fault::unavailable(
        request.phase(),
        crate::real_controller::Unavailable::Backend,
    ))
}
