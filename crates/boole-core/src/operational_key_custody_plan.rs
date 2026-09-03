//! Public, side-effect-free validation for an operational key-custody plan.
//!
//! This contract records who is intended to hold each release/recovery role
//! and where the public bootstrap and its independent root pin will be
//! published. It accepts no key material and grants no signing, release, or
//! activation authority.

use std::collections::BTreeSet;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::release_contract_util::{self, ContractJsonError};

pub const OPERATIONAL_KEY_CUSTODY_PLAN_SCHEMA: &str = "boole.operational-key-custody-plan.v1";
pub const OPERATIONAL_KEY_CUSTODY_PLAN_ENVIRONMENT: &str = "operational-production-readiness";
pub const MAX_OPERATIONAL_KEY_CUSTODY_PLAN_BYTES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedOperationalKeyCustodyPlan {
    plan_id: String,
    plan_sha256: String,
    assignment_count: usize,
    recovery_custodian_count: usize,
    publication_hosts: [String; 2],
    operator_approval_id: String,
}

impl VerifiedOperationalKeyCustodyPlan {
    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }

    pub fn plan_sha256(&self) -> &str {
        &self.plan_sha256
    }

    pub fn assignment_count(&self) -> usize {
        self.assignment_count
    }

    pub fn recovery_custodian_count(&self) -> usize {
        self.recovery_custodian_count
    }

    pub fn publication_hosts(&self) -> &[String; 2] {
        &self.publication_hosts
    }

    pub fn operator_approval_id(&self) -> &str {
        &self.operator_approval_id
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OperationalKeyCustodyPlanError {
    #[error("malformed operational key-custody plan: {0}")]
    Malformed(String),
    #[error("operational key-custody plan must be canonical JSON")]
    NonCanonical,
    #[error("operational key-custody plan rejected: {0}")]
    Rejected(String),
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Role {
    ProductRelease,
    GuestRelease,
    RecoveryA,
    RecoveryB,
    RecoveryC,
}

impl Role {
    fn label(self) -> &'static str {
        match self {
            Self::ProductRelease => "product-release",
            Self::GuestRelease => "guest-release",
            Self::RecoveryA => "recovery-a",
            Self::RecoveryB => "recovery-b",
            Self::RecoveryC => "recovery-c",
        }
    }

    fn is_recovery(self) -> bool {
        matches!(self, Self::RecoveryA | Self::RecoveryB | Self::RecoveryC)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum CustodyClass {
    OnlineSigning,
    OfflineRecovery,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum DeviceClass {
    DedicatedOnlineSigner,
    OfflineRemovableMedia,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Assignment {
    role: Role,
    custody_class: CustodyClass,
    custodian_id: String,
    device_id: String,
    device_class: DeviceClass,
    site_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublicationChannel {
    channel_id: String,
    control_domain_id: String,
    https_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Publication {
    bootstrap: PublicationChannel,
    recovery_root_pin: PublicationChannel,
    root_pin_format: String,
    root_pin_must_precede_adoption: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Controls {
    private_keys_forbidden_from_repository: bool,
    recovery_devices_remain_offline: bool,
    ceremony_needs_two_recovery_custodians: bool,
    production_activation_excluded: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Approval {
    operator_id: String,
    approval_id: String,
    scope: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Plan {
    schema: String,
    plan_id: String,
    environment: String,
    assignments: Vec<Assignment>,
    publication: Publication,
    approval: Approval,
    controls: Controls,
}

/// Validate an operator-authored plan without touching private keys, files,
/// networks, release state, or activation state.
pub fn verify_operational_key_custody_plan(
    raw: &[u8],
) -> Result<VerifiedOperationalKeyCustodyPlan, OperationalKeyCustodyPlanError> {
    let value = release_contract_util::parse_canonical_json(
        "operational key-custody plan",
        raw,
        MAX_OPERATIONAL_KEY_CUSTODY_PLAN_BYTES,
    )
    .map_err(|error| match error {
        ContractJsonError::Malformed(message) => OperationalKeyCustodyPlanError::Malformed(message),
        ContractJsonError::NonCanonical(_) => OperationalKeyCustodyPlanError::NonCanonical,
    })?;
    let plan: Plan = serde_json::from_value(value)
        .map_err(|error| OperationalKeyCustodyPlanError::Malformed(error.to_string()))?;

    if plan.schema != OPERATIONAL_KEY_CUSTODY_PLAN_SCHEMA {
        return Err(OperationalKeyCustodyPlanError::Malformed(
            "schema mismatch".to_string(),
        ));
    }
    safe_id("planId", &plan.plan_id)?;
    if plan.environment != OPERATIONAL_KEY_CUSTODY_PLAN_ENVIRONMENT {
        return Err(OperationalKeyCustodyPlanError::Rejected(
            "environment must be operational-production-readiness".to_string(),
        ));
    }

    let expected_roles = [
        Role::ProductRelease,
        Role::GuestRelease,
        Role::RecoveryA,
        Role::RecoveryB,
        Role::RecoveryC,
    ];
    if plan.assignments.len() != expected_roles.len() {
        return Err(OperationalKeyCustodyPlanError::Rejected(
            "the plan needs exactly five custody assignments".to_string(),
        ));
    }

    let mut all_devices = BTreeSet::new();
    let mut recovery_custodians = BTreeSet::new();
    let mut recovery_sites = BTreeSet::new();
    let mut online_custodians = BTreeSet::new();
    for (assignment, expected_role) in plan.assignments.iter().zip(expected_roles) {
        if assignment.role != expected_role {
            return Err(OperationalKeyCustodyPlanError::Rejected(format!(
                "custody assignments must be ordered as product-release, guest-release, recovery-a, recovery-b, recovery-c; found {}",
                assignment.role.label()
            )));
        }
        for (name, value) in [
            ("custodianId", assignment.custodian_id.as_str()),
            ("deviceId", assignment.device_id.as_str()),
            ("siteId", assignment.site_id.as_str()),
        ] {
            safe_id(name, value)?;
        }
        if !all_devices.insert(assignment.device_id.as_str()) {
            return Err(OperationalKeyCustodyPlanError::Rejected(
                "each role needs a distinct custody device".to_string(),
            ));
        }
        if assignment.role.is_recovery() {
            if assignment.custody_class != CustodyClass::OfflineRecovery
                || assignment.device_class != DeviceClass::OfflineRemovableMedia
            {
                return Err(OperationalKeyCustodyPlanError::Rejected(
                    "every recovery role must use offline-recovery custody on offline removable media"
                        .to_string(),
                ));
            }
            recovery_custodians.insert(assignment.custodian_id.as_str());
            recovery_sites.insert(assignment.site_id.as_str());
        } else {
            if assignment.custody_class != CustodyClass::OnlineSigning
                || assignment.device_class != DeviceClass::DedicatedOnlineSigner
            {
                return Err(OperationalKeyCustodyPlanError::Rejected(
                    "product and guest release roles must use dedicated online-signing devices"
                        .to_string(),
                ));
            }
            online_custodians.insert(assignment.custodian_id.as_str());
        }
    }
    if online_custodians.len() != 2 {
        return Err(OperationalKeyCustodyPlanError::Rejected(
            "product and guest release roles need distinct custodians".to_string(),
        ));
    }
    if recovery_custodians.len() != 3 || recovery_sites.len() != 3 {
        return Err(OperationalKeyCustodyPlanError::Rejected(
            "the three recovery roles need distinct custodians and sites".to_string(),
        ));
    }

    safe_id(
        "bootstrap.channelId",
        &plan.publication.bootstrap.channel_id,
    )?;
    safe_id(
        "recoveryRootPin.channelId",
        &plan.publication.recovery_root_pin.channel_id,
    )?;
    safe_id(
        "bootstrap.controlDomainId",
        &plan.publication.bootstrap.control_domain_id,
    )?;
    safe_id(
        "recoveryRootPin.controlDomainId",
        &plan.publication.recovery_root_pin.control_domain_id,
    )?;
    if plan.publication.bootstrap.channel_id == plan.publication.recovery_root_pin.channel_id {
        return Err(OperationalKeyCustodyPlanError::Rejected(
            "bootstrap and recovery-root pin need distinct channel identifiers".to_string(),
        ));
    }
    if plan.publication.bootstrap.control_domain_id
        == plan.publication.recovery_root_pin.control_domain_id
    {
        return Err(OperationalKeyCustodyPlanError::Rejected(
            "bootstrap and recovery-root pin need distinct administrative control domains"
                .to_string(),
        ));
    }
    let bootstrap_host = https_host("bootstrap.httpsUrl", &plan.publication.bootstrap.https_url)?;
    let pin_host = https_host(
        "recoveryRootPin.httpsUrl",
        &plan.publication.recovery_root_pin.https_url,
    )?;
    if bootstrap_host == pin_host {
        return Err(OperationalKeyCustodyPlanError::Rejected(
            "bootstrap and recovery-root pin need distinct HTTPS hosts".to_string(),
        ));
    }
    if plan.publication.root_pin_format != "sha256-lowercase-hex"
        || !plan.publication.root_pin_must_precede_adoption
    {
        return Err(OperationalKeyCustodyPlanError::Rejected(
            "the independent root pin must be lowercase SHA-256 and precede adoption".to_string(),
        ));
    }

    safe_id("approval.operatorId", &plan.approval.operator_id)?;
    safe_id("approval.approvalId", &plan.approval.approval_id)?;
    if plan.approval.scope != "ceremony-preparation-only" {
        return Err(OperationalKeyCustodyPlanError::Rejected(
            "operator approval scope must be ceremony-preparation-only".to_string(),
        ));
    }

    if !plan.controls.private_keys_forbidden_from_repository
        || !plan.controls.recovery_devices_remain_offline
        || !plan.controls.ceremony_needs_two_recovery_custodians
        || !plan.controls.production_activation_excluded
    {
        return Err(OperationalKeyCustodyPlanError::Rejected(
            "all four operational safety controls must remain enabled".to_string(),
        ));
    }

    Ok(VerifiedOperationalKeyCustodyPlan {
        plan_id: plan.plan_id,
        plan_sha256: hex::encode(Sha256::digest(raw)),
        assignment_count: plan.assignments.len(),
        recovery_custodian_count: recovery_custodians.len(),
        publication_hosts: [bootstrap_host, pin_host],
        operator_approval_id: plan.approval.approval_id,
    })
}

fn safe_id(name: &str, value: &str) -> Result<(), OperationalKeyCustodyPlanError> {
    release_contract_util::check_safe_identifier(name, value)
        .map_err(OperationalKeyCustodyPlanError::Malformed)?;
    if value != value.to_ascii_lowercase() {
        return Err(OperationalKeyCustodyPlanError::Malformed(format!(
            "{name} must be lowercase"
        )));
    }
    Ok(())
}

fn https_host(name: &str, value: &str) -> Result<String, OperationalKeyCustodyPlanError> {
    if value.len() > 2_048 || value.contains(['?', '#']) {
        return Err(OperationalKeyCustodyPlanError::Malformed(format!(
            "{name} must be a bounded HTTPS URL without query or fragment"
        )));
    }
    let rest = value.strip_prefix("https://").ok_or_else(|| {
        OperationalKeyCustodyPlanError::Malformed(format!("{name} must use https"))
    })?;
    let (host, path) = rest.split_once('/').ok_or_else(|| {
        OperationalKeyCustodyPlanError::Malformed(format!("{name} needs an explicit path"))
    })?;
    if path.is_empty()
        || host.is_empty()
        || host != host.to_ascii_lowercase()
        || !host.contains('.')
        || host == "localhost"
        || host.contains(['@', ':', '[', ']'])
        || !host.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(OperationalKeyCustodyPlanError::Malformed(format!(
            "{name} needs a lowercase public DNS host"
        )));
    }
    Ok(host.to_string())
}
