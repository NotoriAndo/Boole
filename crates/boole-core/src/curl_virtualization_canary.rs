//! CURL.3-PREP — frozen acceptance contract of the clean-Mac, Team-ID-free
//! virtualization-entitlement canary.
//!
//! Section 14 of the mac-first execution plan makes one question an explicit
//! measured gate: does a host controller signed **without an Apple Team ID**,
//! carrying only `com.apple.security.virtualization` in an ad-hoc signature,
//! actually start and stop a contained Linux guest on a clean supported Mac?
//! This module freezes what an answer must look like. It evaluates a canary
//! report; it never signs, downloads, boots or probes anything, so it stays
//! pure and runs identically on every platform.
//!
//! Frozen fail-closed evaluation order — an earlier step never depends on a
//! later one, so evidence is rejected at the first unmet ground:
//!
//! 1. macOS floor: below the frozen product minimum aborts before anything
//!    else is read;
//! 2. architecture: Apple Silicon only, Intel is outside the v1 range;
//! 3. clean-machine grounds: **a developer machine can never yield a pass**,
//!    checked before any success signal so a good run on a dirty host is
//!    never miscounted as clean-Mac evidence;
//! 4. signing form: the ad-hoc, Team-ID-free signature is the subject of the
//!    experiment — a Team-ID signature answers a different question and an
//!    unsigned binary carries no entitlement at all;
//! 5. entitlement: `com.apple.security.virtualization` must be present in
//!    that signature;
//! 6. execution mode: only an entitled, isolated VM counts. An unentitled
//!    fallback or a non-isolated host process is a rejection, never a
//!    degraded pass;
//! 7. boot loader: CURL.1 froze direct kernel boot, so the EFI path is
//!    rejected here too;
//! 8. boot inputs: exactly the three frozen roles, in frozen order, each
//!    non-empty and pinned by lowercase hex SHA-256, and the reboot must
//!    reuse byte-identical pins — a canary that re-fetches different inputs
//!    proves nothing about a fixed guest;
//! 9. lifecycle: exactly the boot, shutdown and reboot boundaries, in order,
//!    each completed;
//! 10. residue: every boundary must leave no file and no process behind.
//!
//! What this module deliberately does not do: decide that a given machine is
//! clean. Cleanliness enters as explicit operator-established grounds, and
//! every unestablished ground is named in the rejection.

use std::fmt;

use thiserror::Error;

/// The entitlement the signed host controller must carry to call
/// Virtualization.framework.
pub const CURL_CANARY_REQUIRED_ENTITLEMENT: &str = "com.apple.security.virtualization";

/// The frozen macOS floor of the canary, mirroring the product minimum.
pub const CURL_CANARY_MINIMUM_MACOS: MacOsVersion = MacOsVersion::new(14, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MacOsVersion {
    major: u32,
    minor: u32,
}

impl MacOsVersion {
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    pub const fn major(self) -> u32 {
        self.major
    }

    pub const fn minor(self) -> u32 {
        self.minor
    }
}

impl fmt::Display for MacOsVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostArchitecture {
    AppleSilicon,
    Intel,
}

/// The three host-side boot inputs frozen by the CURL.1 direct-kernel-boot
/// decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanaryBootInputRole {
    GuestKernel,
    GuestInitrd,
    GuestRootDisk,
}

impl CanaryBootInputRole {
    pub const ALL: [Self; 3] = [Self::GuestKernel, Self::GuestInitrd, Self::GuestRootDisk];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GuestKernel => "guest-kernel",
            Self::GuestInitrd => "guest-initrd",
            Self::GuestRootDisk => "guest-root-disk",
        }
    }
}

impl fmt::Display for CanaryBootInputRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanaryBootInputPin {
    role: CanaryBootInputRole,
    file_name: String,
    byte_length: u64,
    sha256: String,
}

impl CanaryBootInputPin {
    pub fn new(role: CanaryBootInputRole, file_name: &str, byte_length: u64, sha256: &str) -> Self {
        Self {
            role,
            file_name: file_name.to_string(),
            byte_length,
            sha256: sha256.to_string(),
        }
    }

    pub fn role(&self) -> CanaryBootInputRole {
        self.role
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub fn byte_length(&self) -> u64 {
        self.byte_length
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanarySigningForm {
    /// Team-ID-free ad-hoc signature — the subject of this experiment.
    AdHoc,
    TeamIdentity {
        team_id: String,
    },
    Unsigned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanaryHostSignature {
    form: CanarySigningForm,
    entitlements: Vec<String>,
}

impl CanaryHostSignature {
    pub fn ad_hoc(entitlements: &[&str]) -> Self {
        Self {
            form: CanarySigningForm::AdHoc,
            entitlements: entitlements.iter().map(|key| key.to_string()).collect(),
        }
    }

    pub fn team_identity(team_id: &str, entitlements: &[&str]) -> Self {
        Self {
            form: CanarySigningForm::TeamIdentity {
                team_id: team_id.to_string(),
            },
            entitlements: entitlements.iter().map(|key| key.to_string()).collect(),
        }
    }

    pub fn unsigned() -> Self {
        Self {
            form: CanarySigningForm::Unsigned,
            entitlements: Vec::new(),
        }
    }

    pub fn form(&self) -> &CanarySigningForm {
        &self.form
    }

    pub fn entitlements(&self) -> &[String] {
        &self.entitlements
    }
}

/// Operator-established grounds that the canary host is a clean supported
/// Mac. Every ground is explicit: nothing is inferred from the fact that the
/// canary ran successfully.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanMachineEvidence {
    operator_attested_erase_install: bool,
    developer_toolchain_absent: bool,
    boole_source_tree_absent: bool,
    prior_boole_install_absent: bool,
}

impl CleanMachineEvidence {
    pub const fn new(
        operator_attested_erase_install: bool,
        developer_toolchain_absent: bool,
        boole_source_tree_absent: bool,
        prior_boole_install_absent: bool,
    ) -> Self {
        Self {
            operator_attested_erase_install,
            developer_toolchain_absent,
            boole_source_tree_absent,
            prior_boole_install_absent,
        }
    }

    /// Every ground established: an erased-and-reinstalled Mac that has never
    /// carried a developer toolchain, the Boole source tree or an earlier
    /// Boole install.
    pub const fn erased_and_clean() -> Self {
        Self::new(true, true, true, true)
    }

    /// The machine this repository is developed on: not erase-installed, with
    /// the toolchain and source tree present. Kept as a named constructor so
    /// developer-machine runs are recorded honestly instead of being dressed
    /// up as clean evidence.
    pub const fn developer_machine() -> Self {
        Self::new(false, false, false, true)
    }

    pub fn missing_grounds(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if !self.operator_attested_erase_install {
            missing.push("erase-install");
        }
        if !self.developer_toolchain_absent {
            missing.push("developer-toolchain-absent");
        }
        if !self.boole_source_tree_absent {
            missing.push("boole-source-tree-absent");
        }
        if !self.prior_boole_install_absent {
            missing.push("prior-boole-install-absent");
        }
        missing
    }

    pub fn is_clean(&self) -> bool {
        self.missing_grounds().is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanaryExecutionMode {
    /// The entitled host controller drove a Virtualization.framework guest.
    EntitledIsolatedVm,
    /// The entitlement was missing or refused and the run continued anyway.
    UnentitledFallback,
    /// The workload ran directly on the host instead of inside a guest.
    NonIsolatedHostProcess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanaryBootLoader {
    VzLinuxBootLoader,
    VzEfiBootLoader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanaryLifecyclePhase {
    Boot,
    Shutdown,
    Reboot,
}

impl CanaryLifecyclePhase {
    pub const ALL: [Self; 3] = [Self::Boot, Self::Shutdown, Self::Reboot];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Shutdown => "shutdown",
            Self::Reboot => "reboot",
        }
    }
}

impl fmt::Display for CanaryLifecyclePhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanaryPhaseOutcome {
    Completed,
    Failed(String),
}

/// What the host still carried after a boundary: any entry is residue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanaryResidueScan {
    leftover_paths: Vec<String>,
    leftover_processes: Vec<String>,
}

impl CanaryResidueScan {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with_paths(paths: &[&str]) -> Self {
        Self {
            leftover_paths: paths.iter().map(|path| path.to_string()).collect(),
            leftover_processes: Vec::new(),
        }
    }

    pub fn with_processes(processes: &[&str]) -> Self {
        Self {
            leftover_paths: Vec::new(),
            leftover_processes: processes.iter().map(|name| name.to_string()).collect(),
        }
    }

    pub fn leftover_paths(&self) -> &[String] {
        &self.leftover_paths
    }

    pub fn leftover_processes(&self) -> &[String] {
        &self.leftover_processes
    }

    pub fn is_empty(&self) -> bool {
        self.leftover_paths.is_empty() && self.leftover_processes.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanaryPhaseRecord {
    phase: CanaryLifecyclePhase,
    outcome: CanaryPhaseOutcome,
    residue_after: CanaryResidueScan,
}

impl CanaryPhaseRecord {
    pub fn completed(phase: CanaryLifecyclePhase, residue_after: CanaryResidueScan) -> Self {
        Self {
            phase,
            outcome: CanaryPhaseOutcome::Completed,
            residue_after,
        }
    }

    pub fn failed(
        phase: CanaryLifecyclePhase,
        reason: &str,
        residue_after: CanaryResidueScan,
    ) -> Self {
        Self {
            phase,
            outcome: CanaryPhaseOutcome::Failed(reason.to_string()),
            residue_after,
        }
    }

    pub fn phase(&self) -> CanaryLifecyclePhase {
        self.phase
    }

    pub fn outcome(&self) -> &CanaryPhaseOutcome {
        &self.outcome
    }

    pub fn residue_after(&self) -> &CanaryResidueScan {
        &self.residue_after
    }
}

/// One canary run, as observed and attested by the operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurlVirtualizationCanaryReport {
    pub macos_version: MacOsVersion,
    pub architecture: HostArchitecture,
    pub clean_machine_evidence: CleanMachineEvidence,
    pub signature: CanaryHostSignature,
    pub execution_mode: CanaryExecutionMode,
    pub boot_loader: CanaryBootLoader,
    pub boot_inputs: Vec<CanaryBootInputPin>,
    pub reboot_boot_inputs: Vec<CanaryBootInputPin>,
    pub lifecycle: Vec<CanaryPhaseRecord>,
}

/// A canary report that met every frozen ground.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurlVirtualizationCanaryPass {
    macos_version: MacOsVersion,
    boot_inputs: Vec<CanaryBootInputPin>,
}

impl CurlVirtualizationCanaryPass {
    pub fn macos_version(&self) -> MacOsVersion {
        self.macos_version
    }

    pub fn boot_inputs(&self) -> &[CanaryBootInputPin] {
        &self.boot_inputs
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CurlVirtualizationCanaryRejection {
    #[error("unsupported macOS: {0}")]
    UnsupportedMacOs(String),
    #[error("unsupported architecture: {0}")]
    UnsupportedArchitecture(String),
    #[error("host is not a clean supported Mac: {0}")]
    MachineNotClean(String),
    #[error("signing form rejected: {0}")]
    SigningFormRejected(String),
    #[error("entitlement missing: {0}")]
    EntitlementMissing(String),
    #[error("execution mode rejected: {0}")]
    ExecutionModeRejected(String),
    #[error("boot loader rejected: {0}")]
    BootLoaderRejected(String),
    #[error("boot inputs rejected: {0}")]
    BootInputsRejected(String),
    #[error("canary lifecycle incomplete: {0}")]
    LifecycleIncomplete(String),
    #[error("residue present: {0}")]
    ResiduePresent(String),
}

/// Evaluate one canary report against the frozen contract, in the fail-closed
/// order documented at the top of this module.
pub fn evaluate_curl_virtualization_canary(
    report: &CurlVirtualizationCanaryReport,
) -> Result<CurlVirtualizationCanaryPass, CurlVirtualizationCanaryRejection> {
    check_macos_version(report.macos_version)?;
    check_architecture(report.architecture)?;
    check_clean_machine(&report.clean_machine_evidence)?;
    check_signature(&report.signature)?;
    check_execution_mode(report.execution_mode)?;
    check_boot_loader(report.boot_loader)?;
    check_boot_inputs(&report.boot_inputs, &report.reboot_boot_inputs)?;
    check_lifecycle(&report.lifecycle)?;
    check_residue(&report.lifecycle)?;

    Ok(CurlVirtualizationCanaryPass {
        macos_version: report.macos_version,
        boot_inputs: report.boot_inputs.clone(),
    })
}

fn check_macos_version(version: MacOsVersion) -> Result<(), CurlVirtualizationCanaryRejection> {
    if version < CURL_CANARY_MINIMUM_MACOS {
        return Err(CurlVirtualizationCanaryRejection::UnsupportedMacOs(
            format!("macOS {version} is below the frozen {CURL_CANARY_MINIMUM_MACOS} canary floor"),
        ));
    }
    Ok(())
}

fn check_architecture(
    architecture: HostArchitecture,
) -> Result<(), CurlVirtualizationCanaryRejection> {
    match architecture {
        HostArchitecture::AppleSilicon => Ok(()),
        HostArchitecture::Intel => Err(CurlVirtualizationCanaryRejection::UnsupportedArchitecture(
            "Intel Macs are outside the frozen Apple Silicon (M1 and later) canary range"
                .to_string(),
        )),
    }
}

fn check_clean_machine(
    evidence: &CleanMachineEvidence,
) -> Result<(), CurlVirtualizationCanaryRejection> {
    let missing = evidence.missing_grounds();
    if missing.is_empty() {
        return Ok(());
    }
    Err(CurlVirtualizationCanaryRejection::MachineNotClean(format!(
        "unestablished grounds: {}. A successful run on a developer machine is not clean-Mac evidence",
        missing.join(", ")
    )))
}

fn check_signature(
    signature: &CanaryHostSignature,
) -> Result<(), CurlVirtualizationCanaryRejection> {
    match signature.form() {
        CanarySigningForm::AdHoc => {}
        CanarySigningForm::TeamIdentity { team_id } => {
            return Err(CurlVirtualizationCanaryRejection::SigningFormRejected(
                format!(
                    "the canary must exercise the Team-ID-free ad-hoc form; \
                     team identity {team_id} answers a different question"
                ),
            ));
        }
        CanarySigningForm::Unsigned => {
            return Err(CurlVirtualizationCanaryRejection::SigningFormRejected(
                "an unsigned host controller carries no entitlement; the canary requires an \
                 ad-hoc signature"
                    .to_string(),
            ));
        }
    }

    if !signature
        .entitlements()
        .iter()
        .any(|key| key == CURL_CANARY_REQUIRED_ENTITLEMENT)
    {
        return Err(CurlVirtualizationCanaryRejection::EntitlementMissing(
            format!("the ad-hoc signature does not carry {CURL_CANARY_REQUIRED_ENTITLEMENT}"),
        ));
    }

    Ok(())
}

fn check_execution_mode(
    mode: CanaryExecutionMode,
) -> Result<(), CurlVirtualizationCanaryRejection> {
    match mode {
        CanaryExecutionMode::EntitledIsolatedVm => Ok(()),
        CanaryExecutionMode::UnentitledFallback => {
            Err(CurlVirtualizationCanaryRejection::ExecutionModeRejected(
                "the run continued without the virtualization entitlement; an unentitled \
                 fallback is a failure, never a degraded pass"
                    .to_string(),
            ))
        }
        CanaryExecutionMode::NonIsolatedHostProcess => {
            Err(CurlVirtualizationCanaryRejection::ExecutionModeRejected(
                "the workload ran directly on the host; a non-isolated run proves nothing about \
                 guest containment"
                    .to_string(),
            ))
        }
    }
}

fn check_boot_loader(
    boot_loader: CanaryBootLoader,
) -> Result<(), CurlVirtualizationCanaryRejection> {
    match boot_loader {
        CanaryBootLoader::VzLinuxBootLoader => Ok(()),
        CanaryBootLoader::VzEfiBootLoader => {
            Err(CurlVirtualizationCanaryRejection::BootLoaderRejected(
                "CURL.1 froze direct kernel boot with VZLinuxBootLoader; VZEFIBootLoader is \
                 rejected because it needs an in-guest bootloader and a mutable variable store"
                    .to_string(),
            ))
        }
    }
}

fn check_boot_inputs(
    boot_inputs: &[CanaryBootInputPin],
    reboot_boot_inputs: &[CanaryBootInputPin],
) -> Result<(), CurlVirtualizationCanaryRejection> {
    for (index, expected_role) in CanaryBootInputRole::ALL.iter().enumerate() {
        let Some(pin) = boot_inputs.get(index) else {
            return Err(CurlVirtualizationCanaryRejection::BootInputsRejected(
                format!(
                    "boot input {} must pin {expected_role} but none was supplied",
                    index + 1
                ),
            ));
        };
        if pin.role() != *expected_role {
            return Err(CurlVirtualizationCanaryRejection::BootInputsRejected(
                format!(
                    "boot input {} must pin {expected_role} but {} was supplied",
                    index + 1,
                    pin.role()
                ),
            ));
        }
        check_boot_input_pin(pin)?;
    }

    if boot_inputs.len() > CanaryBootInputRole::ALL.len() {
        return Err(CurlVirtualizationCanaryRejection::BootInputsRejected(
            format!(
                "the canary pins exactly {} boot inputs but {} were supplied",
                CanaryBootInputRole::ALL.len(),
                boot_inputs.len()
            ),
        ));
    }

    if reboot_boot_inputs != boot_inputs {
        return Err(CurlVirtualizationCanaryRejection::BootInputsRejected(
            "the reboot did not reuse the identical fixed boot inputs; a canary that re-fetches \
             different inputs proves nothing about a fixed guest"
                .to_string(),
        ));
    }

    Ok(())
}

fn check_boot_input_pin(pin: &CanaryBootInputPin) -> Result<(), CurlVirtualizationCanaryRejection> {
    if pin.byte_length() == 0 {
        return Err(CurlVirtualizationCanaryRejection::BootInputsRejected(
            format!("{} is pinned at zero bytes", pin.role()),
        ));
    }
    let digest = pin.sha256();
    if digest.len() != 64
        || !digest
            .chars()
            .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character))
    {
        return Err(CurlVirtualizationCanaryRejection::BootInputsRejected(
            format!(
                "{} is not pinned by a lowercase hex SHA-256 digest",
                pin.role()
            ),
        ));
    }
    Ok(())
}

fn check_lifecycle(
    lifecycle: &[CanaryPhaseRecord],
) -> Result<(), CurlVirtualizationCanaryRejection> {
    for (index, expected_phase) in CanaryLifecyclePhase::ALL.iter().enumerate() {
        let Some(record) = lifecycle.get(index) else {
            return Err(CurlVirtualizationCanaryRejection::LifecycleIncomplete(
                format!("the {expected_phase} boundary was never recorded"),
            ));
        };
        if record.phase() != *expected_phase {
            return Err(CurlVirtualizationCanaryRejection::LifecycleIncomplete(
                format!(
                    "boundary {} must be {expected_phase} but {} was recorded",
                    index + 1,
                    record.phase()
                ),
            ));
        }
        if let CanaryPhaseOutcome::Failed(reason) = record.outcome() {
            return Err(CurlVirtualizationCanaryRejection::LifecycleIncomplete(
                format!("the {expected_phase} boundary failed: {reason}"),
            ));
        }
    }

    if lifecycle.len() > CanaryLifecyclePhase::ALL.len() {
        return Err(CurlVirtualizationCanaryRejection::LifecycleIncomplete(
            format!(
                "the minimal canary records exactly {} boundaries but {} were recorded",
                CanaryLifecyclePhase::ALL.len(),
                lifecycle.len()
            ),
        ));
    }

    Ok(())
}

fn check_residue(lifecycle: &[CanaryPhaseRecord]) -> Result<(), CurlVirtualizationCanaryRejection> {
    for record in lifecycle {
        let residue = record.residue_after();
        if residue.is_empty() {
            continue;
        }
        let mut leftovers = Vec::new();
        leftovers.extend(residue.leftover_paths().iter().cloned());
        leftovers.extend(residue.leftover_processes().iter().cloned());
        return Err(CurlVirtualizationCanaryRejection::ResiduePresent(format!(
            "the {} boundary left {}",
            record.phase(),
            leftovers.join(", ")
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curl_product_release::CURL_PRODUCT_RELEASE_MINIMUM_MACOS;

    #[test]
    fn the_canary_floor_mirrors_the_frozen_product_minimum() {
        assert_eq!(
            CURL_CANARY_MINIMUM_MACOS.to_string(),
            CURL_PRODUCT_RELEASE_MINIMUM_MACOS
        );
    }

    #[test]
    fn a_developer_machine_never_reports_itself_as_clean() {
        assert!(!CleanMachineEvidence::developer_machine().is_clean());
        assert!(CleanMachineEvidence::erased_and_clean().is_clean());
    }
}
