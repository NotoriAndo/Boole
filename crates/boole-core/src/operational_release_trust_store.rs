//! Durable client-side trust-policy chain for installed curl products.
//!
//! The out-of-band recovery root is established once. Every accepted policy
//! generation is retained as immutable public metadata, while one canonical
//! state record atomically selects the trusted chain head. Runtime consumers
//! reconstruct and verify the complete chain before using either online
//! release root.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::canonicalize;
use crate::operational_release_trust_policy::{
    verify_initial_operational_release_trust_policy,
    verify_operational_release_trust_policy_successor, OperationalReleaseRecoveryRoot,
    OperationalReleaseTrustPolicyError, VerifiedOperationalReleaseTrustPolicy,
    MAX_OPERATIONAL_RELEASE_RECOVERY_ROOT_BYTES, MAX_OPERATIONAL_RELEASE_TRUST_POLICY_BYTES,
    MAX_OPERATIONAL_RELEASE_TRUST_POLICY_SIGNATURES_BYTES,
};
use crate::release_contract_util::{self, ContractJsonError};

pub const OPERATIONAL_RELEASE_TRUST_STATE_SCHEMA: &str = "boole.operational-release-trust-state.v1";
pub const OPERATIONAL_RELEASE_TRUST_STATE_FILE: &str = "installed-trust-policy.json";
pub const OPERATIONAL_RELEASE_TRUST_STATE_TEMP_FILE: &str = "installed-trust-policy.json.tmp";
pub const OPERATIONAL_RELEASE_RECOVERY_ROOT_FILE: &str = "operational-recovery-root.json";
pub const OPERATIONAL_RELEASE_TRUST_POLICIES_DIRECTORY: &str = "trust-policies";
pub const OPERATIONAL_RELEASE_TRUST_POLICY_FILE: &str = "trust-policy.json";
pub const OPERATIONAL_RELEASE_TRUST_POLICY_SIGNATURES_FILE: &str = "trust-policy-signatures.json";
const OPERATIONAL_RELEASE_TRUST_STAGING_DIRECTORY: &str = "trust-policy-staging";
const MAX_OPERATIONAL_RELEASE_TRUST_STATE_BYTES: usize = 65_536;
const MAX_POLICY_GENERATIONS: usize = 64;

#[derive(Debug, Error)]
pub enum OperationalReleaseTrustStoreError {
    #[error("trust policy rejected: {0}")]
    Verify(#[from] OperationalReleaseTrustPolicyError),
    #[error("installed trust policy state rejected: {0}")]
    State(String),
    #[error("installed trust policy filesystem operation failed: {0}")]
    Io(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredPolicyIdentity {
    generation: u64,
    policy_sha256: String,
    previous_policy_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledOperationalReleaseTrustPolicyState {
    recovery_root_sha256: String,
    policies: Vec<StoredPolicyIdentity>,
}

impl InstalledOperationalReleaseTrustPolicyState {
    pub fn generation(&self) -> u64 {
        self.policies
            .last()
            .expect("a validated chain is non-empty")
            .generation
    }

    pub fn policy_sha256(&self) -> &str {
        &self
            .policies
            .last()
            .expect("a validated chain is non-empty")
            .policy_sha256
    }

    pub fn previous_policy_sha256(&self) -> Option<&str> {
        self.policies
            .last()
            .expect("a validated chain is non-empty")
            .previous_policy_sha256
            .as_deref()
    }

    pub fn recovery_root_sha256(&self) -> &str {
        &self.recovery_root_sha256
    }

    pub fn policy_count(&self) -> usize {
        self.policies.len()
    }
}

#[derive(Debug)]
pub struct InstalledOperationalReleaseTrustPolicy {
    state: InstalledOperationalReleaseTrustPolicyState,
    verified_policy: VerifiedOperationalReleaseTrustPolicy,
    policy_path: PathBuf,
    policy_directory_count: u64,
    unreferenced_policy_directory_count: u64,
}

impl InstalledOperationalReleaseTrustPolicy {
    pub fn state(&self) -> &InstalledOperationalReleaseTrustPolicyState {
        &self.state
    }

    pub fn verified_policy(&self) -> &VerifiedOperationalReleaseTrustPolicy {
        &self.verified_policy
    }

    pub fn policy_path(&self) -> &Path {
        &self.policy_path
    }

    pub fn policy_directory_count(&self) -> u64 {
        self.policy_directory_count
    }

    pub fn unreferenced_policy_directory_count(&self) -> u64 {
        self.unreferenced_policy_directory_count
    }
}

#[derive(Debug)]
pub struct PreparedOperationalReleaseTrustPolicy {
    state: InstalledOperationalReleaseTrustPolicyState,
    verified_policy: VerifiedOperationalReleaseTrustPolicy,
    recovery_root_raw: Option<Vec<u8>>,
    policy_raw: Option<Vec<u8>>,
    signatures_raw: Option<Vec<u8>>,
}

impl PreparedOperationalReleaseTrustPolicy {
    pub fn verified_policy(&self) -> &VerifiedOperationalReleaseTrustPolicy {
        &self.verified_policy
    }

    pub fn needs_adoption(&self) -> bool {
        self.policy_raw.is_some()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TrustStateWire {
    schema: String,
    recovery_root_sha256: String,
    policies: Vec<StoredPolicyIdentity>,
}

/// Prepare either the initial policy, the exact next policy, or the current
/// installed policy. No filesystem mutation occurs here.
pub fn prepare_operational_release_trust_policy_update(
    install_root: &Path,
    recovery_root_raw: Option<&[u8]>,
    policy_raw: Option<&[u8]>,
    signatures_raw: Option<&[u8]>,
) -> Result<PreparedOperationalReleaseTrustPolicy, OperationalReleaseTrustStoreError> {
    match read_trust_state(install_root)? {
        None => {
            let (Some(recovery_root_raw), Some(policy_raw), Some(signatures_raw)) =
                (recovery_root_raw, policy_raw, signatures_raw)
            else {
                return Err(OperationalReleaseTrustStoreError::State(
                    "first policy adoption requires recovery root, policy and signatures"
                        .to_string(),
                ));
            };
            let recovery = OperationalReleaseRecoveryRoot::from_canonical_json(recovery_root_raw)?;
            let verified = verify_initial_operational_release_trust_policy(
                policy_raw,
                signatures_raw,
                &recovery,
            )?;
            let identity = StoredPolicyIdentity {
                generation: verified.generation(),
                policy_sha256: verified.policy_sha256().to_string(),
                previous_policy_sha256: None,
            };
            Ok(PreparedOperationalReleaseTrustPolicy {
                state: InstalledOperationalReleaseTrustPolicyState {
                    recovery_root_sha256: sha256(recovery_root_raw),
                    policies: vec![identity],
                },
                verified_policy: verified,
                recovery_root_raw: Some(recovery_root_raw.to_vec()),
                policy_raw: Some(policy_raw.to_vec()),
                signatures_raw: Some(signatures_raw.to_vec()),
            })
        }
        Some(_) => {
            if recovery_root_raw.is_some() {
                return Err(OperationalReleaseTrustStoreError::State(
                    "the recovery root is bootstrap-only and cannot be replaced in-band"
                        .to_string(),
                ));
            }
            let installed = open_installed_operational_release_trust_policy(install_root)?;
            match (policy_raw, signatures_raw) {
                (None, None) => Ok(PreparedOperationalReleaseTrustPolicy {
                    state: installed.state,
                    verified_policy: installed.verified_policy,
                    recovery_root_raw: None,
                    policy_raw: None,
                    signatures_raw: None,
                }),
                (Some(policy_raw), Some(signatures_raw)) => {
                    let verified = verify_operational_release_trust_policy_successor(
                        &installed.verified_policy,
                        policy_raw,
                        signatures_raw,
                    )?;
                    let mut state = installed.state;
                    state.policies.push(StoredPolicyIdentity {
                        generation: verified.generation(),
                        policy_sha256: verified.policy_sha256().to_string(),
                        previous_policy_sha256: Some(
                            state
                                .policies
                                .last()
                                .expect("installed policy chain")
                                .policy_sha256
                                .clone(),
                        ),
                    });
                    validate_state(&state)?;
                    Ok(PreparedOperationalReleaseTrustPolicy {
                        state,
                        verified_policy: verified,
                        recovery_root_raw: None,
                        policy_raw: Some(policy_raw.to_vec()),
                        signatures_raw: Some(signatures_raw.to_vec()),
                    })
                }
                _ => Err(OperationalReleaseTrustStoreError::State(
                    "a successor policy and signature set must be supplied together".to_string(),
                )),
            }
        }
    }
}

/// Persist a prepared generation before any release signed by its online
/// roles is accepted. An immutable generation directory is made durable
/// first; one atomic state-file rename then selects it as the chain head.
pub fn adopt_operational_release_trust_policy(
    install_root: &Path,
    prepared: PreparedOperationalReleaseTrustPolicy,
) -> Result<InstalledOperationalReleaseTrustPolicy, OperationalReleaseTrustStoreError> {
    if !prepared.needs_adoption() {
        return open_installed_operational_release_trust_policy(install_root);
    }
    fs::create_dir_all(install_root)
        .map_err(|error| io_error("create install root", install_root, error))?;

    if let Some(recovery_root_raw) = &prepared.recovery_root_raw {
        adopt_bootstrap_recovery_root(
            install_root,
            recovery_root_raw,
            &prepared.state.recovery_root_sha256,
        )?;
    } else {
        verify_recovery_root_file(install_root, &prepared.state.recovery_root_sha256)?;
    }

    let policy_raw = prepared.policy_raw.as_deref().expect("new policy bytes");
    let signatures_raw = prepared
        .signatures_raw
        .as_deref()
        .expect("new policy signatures");
    let identity = prepared
        .state
        .policies
        .last()
        .expect("prepared policy identity");
    adopt_policy_generation(install_root, identity, policy_raw, signatures_raw)?;
    write_trust_state(install_root, &prepared.state)?;
    open_installed_operational_release_trust_policy(install_root)
}

/// Reconstruct and verify the complete persisted chain from the bootstrap
/// recovery root through the state-selected generation.
pub fn open_installed_operational_release_trust_policy(
    install_root: &Path,
) -> Result<InstalledOperationalReleaseTrustPolicy, OperationalReleaseTrustStoreError> {
    let state = read_trust_state(install_root)?.ok_or_else(|| {
        OperationalReleaseTrustStoreError::State(
            "installed trust policy state is absent".to_string(),
        )
    })?;
    let recovery_raw = verify_recovery_root_file(install_root, &state.recovery_root_sha256)?;
    let recovery = OperationalReleaseRecoveryRoot::from_canonical_json(&recovery_raw)?;
    let policies_root = install_root.join(OPERATIONAL_RELEASE_TRUST_POLICIES_DIRECTORY);
    let mut verified = None;
    let mut referenced = std::collections::BTreeSet::new();
    for identity in &state.policies {
        let directory_name = policy_directory_name(identity);
        referenced.insert(directory_name.clone());
        let directory = policies_root.join(&directory_name);
        let directory_metadata = fs::symlink_metadata(&directory).map_err(|error| {
            io_error(
                "inspect installed trust-policy directory",
                &directory,
                error,
            )
        })?;
        if !directory_metadata.is_dir() || directory_metadata.file_type().is_symlink() {
            return Err(OperationalReleaseTrustStoreError::State(
                "installed trust-policy generation is not a real directory".to_string(),
            ));
        }
        let policy_raw = read_bounded_file(
            &directory.join(OPERATIONAL_RELEASE_TRUST_POLICY_FILE),
            MAX_OPERATIONAL_RELEASE_TRUST_POLICY_BYTES,
            "installed trust policy",
        )?;
        if sha256(&policy_raw) != identity.policy_sha256 {
            return Err(OperationalReleaseTrustStoreError::State(
                "installed trust policy digest differs from state".to_string(),
            ));
        }
        let signatures_raw = read_bounded_file(
            &directory.join(OPERATIONAL_RELEASE_TRUST_POLICY_SIGNATURES_FILE),
            MAX_OPERATIONAL_RELEASE_TRUST_POLICY_SIGNATURES_BYTES,
            "installed trust policy signatures",
        )?;
        let current = match verified.as_ref() {
            None => verify_initial_operational_release_trust_policy(
                &policy_raw,
                &signatures_raw,
                &recovery,
            )?,
            Some(previous) => verify_operational_release_trust_policy_successor(
                previous,
                &policy_raw,
                &signatures_raw,
            )?,
        };
        if current.generation() != identity.generation
            || current.policy_sha256() != identity.policy_sha256
        {
            return Err(OperationalReleaseTrustStoreError::State(
                "installed trust policy identity differs from state".to_string(),
            ));
        }
        verified = Some(current);
    }
    let verified_policy = verified.expect("validated state has a policy");
    let policy_path = policies_root
        .join(policy_directory_name(
            state.policies.last().expect("validated policy chain"),
        ))
        .join(OPERATIONAL_RELEASE_TRUST_POLICY_FILE);

    let mut policy_directory_count = 0_u64;
    let mut unreferenced_policy_directory_count = 0_u64;
    for entry in fs::read_dir(&policies_root)
        .map_err(|error| io_error("read trust-policies directory", &policies_root, error))?
    {
        let entry = entry.map_err(|error| {
            OperationalReleaseTrustStoreError::Io(format!(
                "read trust-policies directory entry: {error}"
            ))
        })?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            io_error("inspect trust-policy directory entry", &entry.path(), error)
        })?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            policy_directory_count += 1;
            if !referenced.contains(entry.file_name().to_string_lossy().as_ref()) {
                unreferenced_policy_directory_count += 1;
            }
        }
    }

    Ok(InstalledOperationalReleaseTrustPolicy {
        state,
        verified_policy,
        policy_path,
        policy_directory_count,
        unreferenced_policy_directory_count,
    })
}

fn read_trust_state(
    install_root: &Path,
) -> Result<Option<InstalledOperationalReleaseTrustPolicyState>, OperationalReleaseTrustStoreError>
{
    let path = install_root.join(OPERATIONAL_RELEASE_TRUST_STATE_FILE);
    let raw = match read_optional_bounded_file(
        &path,
        MAX_OPERATIONAL_RELEASE_TRUST_STATE_BYTES,
        "installed trust policy state",
    )? {
        Some(raw) => raw,
        None => return Ok(None),
    };
    let value = release_contract_util::parse_canonical_json(
        OPERATIONAL_RELEASE_TRUST_STATE_FILE,
        &raw,
        MAX_OPERATIONAL_RELEASE_TRUST_STATE_BYTES,
    )
    .map_err(|error| match error {
        ContractJsonError::Malformed(message) => OperationalReleaseTrustStoreError::State(message),
        ContractJsonError::NonCanonical(name) => {
            OperationalReleaseTrustStoreError::State(format!("{name} must be canonical JSON"))
        }
    })?;
    let parsed: TrustStateWire = serde_json::from_value(value)
        .map_err(|error| OperationalReleaseTrustStoreError::State(error.to_string()))?;
    if parsed.schema != OPERATIONAL_RELEASE_TRUST_STATE_SCHEMA {
        return Err(OperationalReleaseTrustStoreError::State(
            "unexpected installed trust policy state schema".to_string(),
        ));
    }
    let state = InstalledOperationalReleaseTrustPolicyState {
        recovery_root_sha256: parsed.recovery_root_sha256,
        policies: parsed.policies,
    };
    validate_state(&state)?;
    Ok(Some(state))
}

fn validate_state(
    state: &InstalledOperationalReleaseTrustPolicyState,
) -> Result<(), OperationalReleaseTrustStoreError> {
    release_contract_util::check_sha256("recoveryRootSha256", &state.recovery_root_sha256)
        .map_err(OperationalReleaseTrustStoreError::State)?;
    if state.policies.is_empty() || state.policies.len() > MAX_POLICY_GENERATIONS {
        return Err(OperationalReleaseTrustStoreError::State(
            "policy chain length is outside its allowed range".to_string(),
        ));
    }
    let mut previous = None;
    for (index, identity) in state.policies.iter().enumerate() {
        let expected_generation = index as u64 + 1;
        if identity.generation != expected_generation {
            return Err(OperationalReleaseTrustStoreError::State(
                "policy generations must begin at one and advance exactly once".to_string(),
            ));
        }
        release_contract_util::check_sha256("policySha256", &identity.policy_sha256)
            .map_err(OperationalReleaseTrustStoreError::State)?;
        if identity.previous_policy_sha256.as_deref() != previous {
            return Err(OperationalReleaseTrustStoreError::State(
                "policy state predecessor chain is discontinuous".to_string(),
            ));
        }
        previous = Some(identity.policy_sha256.as_str());
    }
    Ok(())
}

fn adopt_bootstrap_recovery_root(
    install_root: &Path,
    raw: &[u8],
    expected_sha256: &str,
) -> Result<(), OperationalReleaseTrustStoreError> {
    let path = install_root.join(OPERATIONAL_RELEASE_RECOVERY_ROOT_FILE);
    match read_optional_bounded_file(
        &path,
        MAX_OPERATIONAL_RELEASE_RECOVERY_ROOT_BYTES,
        "installed recovery root",
    )? {
        Some(existing) => {
            if existing != raw || sha256(&existing) != expected_sha256 {
                return Err(OperationalReleaseTrustStoreError::State(
                    "installed recovery root differs from the bootstrap root".to_string(),
                ));
            }
            Ok(())
        }
        None => {
            write_new_durable(&path, raw, 0o444)?;
            sync_dir(install_root)
        }
    }
}

fn verify_recovery_root_file(
    install_root: &Path,
    expected_sha256: &str,
) -> Result<Vec<u8>, OperationalReleaseTrustStoreError> {
    let path = install_root.join(OPERATIONAL_RELEASE_RECOVERY_ROOT_FILE);
    let raw = read_bounded_file(
        &path,
        MAX_OPERATIONAL_RELEASE_RECOVERY_ROOT_BYTES,
        "installed recovery root",
    )?;
    if sha256(&raw) != expected_sha256 {
        return Err(OperationalReleaseTrustStoreError::State(
            "installed recovery root digest differs from state".to_string(),
        ));
    }
    Ok(raw)
}

fn adopt_policy_generation(
    install_root: &Path,
    identity: &StoredPolicyIdentity,
    policy_raw: &[u8],
    signatures_raw: &[u8],
) -> Result<(), OperationalReleaseTrustStoreError> {
    let policies_root = install_root.join(OPERATIONAL_RELEASE_TRUST_POLICIES_DIRECTORY);
    fs::create_dir_all(&policies_root)
        .map_err(|error| io_error("create trust-policies directory", &policies_root, error))?;
    let final_dir = policies_root.join(policy_directory_name(identity));
    if final_dir.exists() {
        let installed_policy = read_bounded_file(
            &final_dir.join(OPERATIONAL_RELEASE_TRUST_POLICY_FILE),
            MAX_OPERATIONAL_RELEASE_TRUST_POLICY_BYTES,
            "existing trust policy",
        )?;
        let installed_signatures = read_bounded_file(
            &final_dir.join(OPERATIONAL_RELEASE_TRUST_POLICY_SIGNATURES_FILE),
            MAX_OPERATIONAL_RELEASE_TRUST_POLICY_SIGNATURES_BYTES,
            "existing trust policy signatures",
        )?;
        if installed_policy != policy_raw || installed_signatures != signatures_raw {
            return Err(OperationalReleaseTrustStoreError::State(
                "existing trust-policy generation differs from prepared bytes".to_string(),
            ));
        }
        return Ok(());
    }
    let staging = install_root.join(OPERATIONAL_RELEASE_TRUST_STAGING_DIRECTORY);
    remove_dir_if_present(&staging)?;
    fs::create_dir(&staging)
        .map_err(|error| io_error("create trust-policy staging directory", &staging, error))?;
    write_new_durable(
        &staging.join(OPERATIONAL_RELEASE_TRUST_POLICY_FILE),
        policy_raw,
        0o444,
    )?;
    write_new_durable(
        &staging.join(OPERATIONAL_RELEASE_TRUST_POLICY_SIGNATURES_FILE),
        signatures_raw,
        0o444,
    )?;
    sync_dir(&staging)?;
    fs::rename(&staging, &final_dir)
        .map_err(|error| io_error("adopt trust-policy generation", &final_dir, error))?;
    fs::set_permissions(&final_dir, fs::Permissions::from_mode(0o555))
        .map_err(|error| io_error("make trust-policy generation read-only", &final_dir, error))?;
    sync_dir(&policies_root)
}

fn write_trust_state(
    install_root: &Path,
    state: &InstalledOperationalReleaseTrustPolicyState,
) -> Result<(), OperationalReleaseTrustStoreError> {
    validate_state(state)?;
    let policies: Vec<_> = state
        .policies
        .iter()
        .map(|identity| {
            json!({
                "generation": identity.generation,
                "policySha256": identity.policy_sha256,
                "previousPolicySha256": identity.previous_policy_sha256,
            })
        })
        .collect();
    let raw = canonicalize(&json!({
        "schema": OPERATIONAL_RELEASE_TRUST_STATE_SCHEMA,
        "recoveryRootSha256": state.recovery_root_sha256,
        "policies": policies,
    }));
    let temp = install_root.join(OPERATIONAL_RELEASE_TRUST_STATE_TEMP_FILE);
    if temp.exists() {
        fs::remove_file(&temp)
            .map_err(|error| io_error("remove stale trust-policy state temp", &temp, error))?;
    }
    write_new_durable(&temp, &raw, 0o444)?;
    let final_path = install_root.join(OPERATIONAL_RELEASE_TRUST_STATE_FILE);
    fs::rename(&temp, &final_path)
        .map_err(|error| io_error("adopt installed trust-policy state", &final_path, error))?;
    sync_dir(install_root)
}

fn read_bounded_file(
    path: &Path,
    cap: usize,
    label: &str,
) -> Result<Vec<u8>, OperationalReleaseTrustStoreError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| io_error(&format!("open {label}"), path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| io_error(&format!("inspect {label}"), path, error))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > cap as u64 {
        return Err(OperationalReleaseTrustStoreError::State(format!(
            "{label} is not a non-empty regular file within its byte cap"
        )));
    }
    let mut raw = Vec::with_capacity(metadata.len() as usize);
    file.take(cap as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|error| io_error(&format!("read {label}"), path, error))?;
    if raw.len() > cap {
        return Err(OperationalReleaseTrustStoreError::State(format!(
            "{label} exceeds its byte cap while being read"
        )));
    }
    Ok(raw)
}

fn read_optional_bounded_file(
    path: &Path,
    cap: usize,
    label: &str,
) -> Result<Option<Vec<u8>>, OperationalReleaseTrustStoreError> {
    match fs::symlink_metadata(path) {
        Ok(_) => read_bounded_file(path, cap, label).map(Some),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(&format!("inspect {label}"), path, error)),
    }
}

fn write_new_durable(
    path: &Path,
    bytes: &[u8],
    mode: u32,
) -> Result<(), OperationalReleaseTrustStoreError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| io_error("create durable trust-policy file", path, error))?;
    file.write_all(bytes)
        .map_err(|error| io_error("write durable trust-policy file", path, error))?;
    file.sync_all()
        .map_err(|error| io_error("sync durable trust-policy file", path, error))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| io_error("set trust-policy file permissions", path, error))?;
    file.sync_all()
        .map_err(|error| io_error("sync trust-policy file metadata", path, error))?;
    Ok(())
}

fn policy_directory_name(identity: &StoredPolicyIdentity) -> String {
    format!(
        "{:012}-{}",
        identity.generation,
        &identity.policy_sha256[..12]
    )
}

fn sha256(raw: &[u8]) -> String {
    hex::encode(Sha256::digest(raw))
}

fn remove_dir_if_present(path: &Path) -> Result<(), OperationalReleaseTrustStoreError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(
            "remove trust-policy staging directory",
            path,
            error,
        )),
    }
}

fn sync_dir(path: &Path) -> Result<(), OperationalReleaseTrustStoreError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync trust-policy directory", path, error))
}

fn io_error(action: &str, path: &Path, error: std::io::Error) -> OperationalReleaseTrustStoreError {
    OperationalReleaseTrustStoreError::Io(format!("{action} at {}: {error}", path.display()))
}
