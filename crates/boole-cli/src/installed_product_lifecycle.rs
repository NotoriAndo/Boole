//! User-facing lifecycle for the installed direct-boot Mac product.
//!
//! The curl installer adopts six host roles and the signed guest release.
//! This module turns that verified installation into one foreground command:
//! it derives all mutable VM/journal paths from one private state root,
//! reopens the active product through both injected trust roots, copies the
//! retained `host-node` descriptor into a private runtime directory, and
//! replaces the CLI process with that verified copy. The installed node then
//! owns SIGINT/SIGTERM and its already-bounded controller shutdown protocol.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use boole_core::{
    open_verified_installed_direct_boot_curl_product_release, CurlProductReleaseTrustRoot,
    NativeShadowUpdateTrustRoot, ProductArtifactRole,
};
use sha2::{Digest, Sha256};

const HOST_NODE_BASENAME: &str = "boole-mac-native-shadow-replay-node";

#[derive(Debug, thiserror::Error)]
pub enum InstalledProductLifecycleError {
    #[error("installed product lifecycle path rejected: {0}")]
    Path(String),
    #[error("installed product lifecycle I/O failed: {0}")]
    Io(String),
    #[error("installed product verification failed: {0}")]
    Verify(String),
    #[error("installed host-node execution failed: {0}")]
    Exec(String),
    #[error("installed product health check failed: {0}")]
    Health(String),
    #[error("installed product lifecycle requires macOS")]
    Unsupported,
}

pub fn default_installed_mac_state_root() -> Result<PathBuf, InstalledProductLifecycleError> {
    let home = std::env::var_os("HOME").ok_or_else(|| {
        InstalledProductLifecycleError::Path(
            "HOME is absent; pass an explicit absolute --state-root".to_string(),
        )
    })?;
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return Err(InstalledProductLifecycleError::Path(
            "HOME is not absolute; pass an explicit absolute --state-root".to_string(),
        ));
    }
    Ok(home.join("Library/Application Support/Boole/native-shadow"))
}

pub fn query_installed_direct_boot_health(
    timeout: std::time::Duration,
) -> Result<serde_json::Value, InstalledProductLifecycleError> {
    query_installed_direct_boot_health_at("http://127.0.0.1:8082", timeout)
}

fn query_installed_direct_boot_health_at(
    base_url: &str,
    timeout: std::time::Duration,
) -> Result<serde_json::Value, InstalledProductLifecycleError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|error| InstalledProductLifecycleError::Health(error.to_string()))?;
    let query = |probe: &str| -> Result<serde_json::Value, InstalledProductLifecycleError> {
        let url = format!("{base_url}/{probe}");
        let mut response = client.get(&url).send().map_err(|error| {
            InstalledProductLifecycleError::Health(format!(
                "{probe} probe could not reach the loopback service: {error}"
            ))
        })?;
        let status = response.status();
        let mut raw = Vec::new();
        response
            .by_ref()
            .take(16 * 1024 + 1)
            .read_to_end(&mut raw)
            .map_err(|error| {
                InstalledProductLifecycleError::Health(format!(
                    "{probe} response could not be read: {error}"
                ))
            })?;
        if raw.len() > 16 * 1024 {
            return Err(InstalledProductLifecycleError::Health(format!(
                "{probe} response exceeds its byte cap"
            )));
        }
        let value: serde_json::Value = serde_json::from_slice(&raw).map_err(|error| {
            InstalledProductLifecycleError::Health(format!("{probe} response is not JSON: {error}"))
        })?;
        if value["schema"] != "boole.native-shadow.service-health.v1"
            || value["probe"] != probe
            || value["loopbackOnly"] != true
            || value["mineableNow"] != false
            || value["activationAllowed"] != false
        {
            return Err(InstalledProductLifecycleError::Health(format!(
                "{probe} response differs from the closed-local health contract"
            )));
        }
        if !status.is_success() {
            return Err(InstalledProductLifecycleError::Health(format!(
                "{probe} probe returned HTTP {}: {}",
                status.as_u16(),
                String::from_utf8_lossy(&raw)
            )));
        }
        Ok(value)
    };
    let live = query("live")?;
    if live["live"] != true {
        return Err(InstalledProductLifecycleError::Health(
            "live probe did not affirm liveness".to_string(),
        ));
    }
    let ready = query("ready")?;
    if ready["ready"] != true {
        return Err(InstalledProductLifecycleError::Health(
            "ready probe did not affirm readiness".to_string(),
        ));
    }
    Ok(serde_json::json!({
        "endpoint": base_url,
        "live": live,
        "ready": ready,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledMacLifecyclePlan {
    state_root: PathBuf,
    controller_runtime_root: PathBuf,
    journal_path: PathBuf,
    host_runtime_root: PathBuf,
    materialized_host_node_path: PathBuf,
}

impl InstalledMacLifecyclePlan {
    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    pub fn controller_runtime_root(&self) -> &Path {
        &self.controller_runtime_root
    }

    pub fn journal_path(&self) -> &Path {
        &self.journal_path
    }

    pub fn materialized_host_node_path(&self) -> &Path {
        &self.materialized_host_node_path
    }
}

pub fn plan_installed_mac_lifecycle(
    state_root: &Path,
) -> Result<InstalledMacLifecyclePlan, InstalledProductLifecycleError> {
    if !state_root.is_absolute() {
        return Err(InstalledProductLifecycleError::Path(
            "state root must be absolute".to_string(),
        ));
    }
    let controller_runtime_root = state_root.join("controller");
    let journal_path = state_root.join("journal/replay.ndjson");
    let host_runtime_root = state_root.join("host");
    let materialized_host_node_path = host_runtime_root.join(HOST_NODE_BASENAME);
    Ok(InstalledMacLifecyclePlan {
        state_root: state_root.to_path_buf(),
        controller_runtime_root,
        journal_path,
        host_runtime_root,
        materialized_host_node_path,
    })
}

#[allow(unsafe_code)]
fn expected_identity() -> (u32, u32) {
    // SAFETY: these calls read immutable process credentials and retain no
    // borrowed OS memory.
    unsafe { (libc::geteuid(), libc::getegid()) }
}

fn ensure_private_directory(
    path: &Path,
    uid: u32,
    gid: u32,
) -> Result<(), InstalledProductLifecycleError> {
    if !path.exists() {
        fs::create_dir_all(path).map_err(|error| {
            InstalledProductLifecycleError::Io(format!(
                "create private directory {}: {error}",
                path.display()
            ))
        })?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            InstalledProductLifecycleError::Io(format!(
                "protect private directory {}: {error}",
                path.display()
            ))
        })?;
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        InstalledProductLifecycleError::Io(format!(
            "inspect private directory {}: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != uid
        || metadata.gid() != gid
        || metadata.permissions().mode() & 0o7777 != 0o700
    {
        return Err(InstalledProductLifecycleError::Path(format!(
            "{} is not an owner-held private 0700 directory",
            path.display()
        )));
    }
    Ok(())
}

fn prepare_plan(
    plan: &InstalledMacLifecyclePlan,
) -> Result<(u32, u32), InstalledProductLifecycleError> {
    let (uid, gid) = expected_identity();
    ensure_private_directory(&plan.state_root, uid, gid)?;
    ensure_private_directory(&plan.controller_runtime_root, uid, gid)?;
    ensure_private_directory(
        plan.journal_path
            .parent()
            .expect("fixed journal path has a parent"),
        uid,
        gid,
    )?;
    ensure_private_directory(&plan.host_runtime_root, uid, gid)?;
    Ok((uid, gid))
}

fn materialize_verified_host_node(
    source: &File,
    expected_len: u64,
    expected_digest: &str,
    plan: &InstalledMacLifecyclePlan,
) -> Result<PathBuf, InstalledProductLifecycleError> {
    let (uid, gid) = prepare_plan(plan)?;
    if expected_digest.len() != 64
        || !expected_digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(InstalledProductLifecycleError::Verify(
            "host-node digest is not lowercase SHA-256".to_string(),
        ));
    }
    let temporary = plan
        .host_runtime_root
        .join(format!(".{HOST_NODE_BASENAME}.{}.tmp", std::process::id()));
    let outcome = (|| {
        let mut target = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&temporary)
            .map_err(|error| {
                InstalledProductLifecycleError::Io(format!(
                    "create host-node runtime copy: {error}"
                ))
            })?;
        let mut source = source.try_clone().map_err(|error| {
            InstalledProductLifecycleError::Io(format!("clone verified host-node: {error}"))
        })?;
        source.seek(SeekFrom::Start(0)).map_err(|error| {
            InstalledProductLifecycleError::Io(format!("rewind verified host-node: {error}"))
        })?;
        let mut hasher = Sha256::new();
        let mut copied = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = source.read(&mut buffer).map_err(|error| {
                InstalledProductLifecycleError::Io(format!("read verified host-node: {error}"))
            })?;
            if read == 0 {
                break;
            }
            copied = copied.checked_add(read as u64).ok_or_else(|| {
                InstalledProductLifecycleError::Verify("host-node length overflowed".to_string())
            })?;
            if copied > expected_len {
                return Err(InstalledProductLifecycleError::Verify(
                    "host-node byte length differs".to_string(),
                ));
            }
            hasher.update(&buffer[..read]);
            target.write_all(&buffer[..read]).map_err(|error| {
                InstalledProductLifecycleError::Io(format!("write host-node copy: {error}"))
            })?;
        }
        if copied != expected_len {
            return Err(InstalledProductLifecycleError::Verify(
                "host-node byte length differs".to_string(),
            ));
        }
        if hex::encode(hasher.finalize()) != expected_digest {
            return Err(InstalledProductLifecycleError::Verify(
                "host-node digest differs".to_string(),
            ));
        }
        target.sync_all().map_err(|error| {
            InstalledProductLifecycleError::Io(format!("sync host-node: {error}"))
        })?;
        target
            .set_permissions(fs::Permissions::from_mode(0o500))
            .map_err(|error| {
                InstalledProductLifecycleError::Io(format!("protect host-node: {error}"))
            })?;
        target.sync_all().map_err(|error| {
            InstalledProductLifecycleError::Io(format!("sync protected host-node: {error}"))
        })?;
        drop(target);
        fs::rename(&temporary, &plan.materialized_host_node_path).map_err(|error| {
            InstalledProductLifecycleError::Io(format!("adopt verified host-node: {error}"))
        })?;
        File::open(&plan.host_runtime_root)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                InstalledProductLifecycleError::Io(format!("sync host runtime directory: {error}"))
            })?;
        let metadata =
            fs::symlink_metadata(&plan.materialized_host_node_path).map_err(|error| {
                InstalledProductLifecycleError::Io(format!("inspect adopted host-node: {error}"))
            })?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.uid() != uid
            || metadata.gid() != gid
            || metadata.nlink() != 1
            || metadata.permissions().mode() & 0o7777 != 0o500
        {
            return Err(InstalledProductLifecycleError::Path(
                "adopted host-node metadata is unsafe".to_string(),
            ));
        }
        Ok(plan.materialized_host_node_path.clone())
    })();
    if outcome.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    outcome
}

#[doc(hidden)]
pub fn materialize_verified_host_node_for_test(
    source: &File,
    expected_len: u64,
    expected_digest: &str,
    plan: &InstalledMacLifecyclePlan,
) -> Result<PathBuf, InstalledProductLifecycleError> {
    materialize_verified_host_node(source, expected_len, expected_digest, plan)
}

#[cfg(target_os = "macos")]
#[allow(clippy::too_many_arguments)]
pub fn run_installed_direct_boot_product(
    install_root: &Path,
    state_root: &Path,
    product_trust_root: &CurlProductReleaseTrustRoot,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
) -> Result<(), InstalledProductLifecycleError> {
    use std::os::unix::process::CommandExt;

    let plan = plan_installed_mac_lifecycle(state_root)?;
    let active = open_verified_installed_direct_boot_curl_product_release(
        install_root,
        product_trust_root,
        guest_trust_root,
    )
    .map_err(|error| InstalledProductLifecycleError::Verify(error.to_string()))?;
    let role = ProductArtifactRole::HostNode;
    let source = active.product().artifact_file(role).ok_or_else(|| {
        InstalledProductLifecycleError::Verify("active product lacks host-node handle".to_string())
    })?;
    let expected_len = active.product().artifact_byte_length(role).ok_or_else(|| {
        InstalledProductLifecycleError::Verify("active product lacks host-node length".to_string())
    })?;
    let expected_digest = active.product().artifact_sha256(role).ok_or_else(|| {
        InstalledProductLifecycleError::Verify("active product lacks host-node digest".to_string())
    })?;
    let executable = materialize_verified_host_node(source, expected_len, expected_digest, &plan)?;

    let error = std::process::Command::new(&executable)
        .arg("--install-root")
        .arg(install_root)
        .arg("--runtime-root")
        .arg(plan.controller_runtime_root())
        .arg("--journal-path")
        .arg(plan.journal_path())
        .arg("--product-trust-root-key-id")
        .arg(product_trust_root.key_id())
        .arg("--product-trust-root-public-key")
        .arg(product_trust_root.public_key_hex())
        .arg("--guest-trust-root-key-id")
        .arg(guest_trust_root.key_id())
        .arg("--guest-trust-root-public-key")
        .arg(guest_trust_root.public_key_hex())
        .current_dir(plan.state_root())
        .env_clear()
        .exec();
    Err(InstalledProductLifecycleError::Exec(error.to_string()))
}

#[cfg(not(target_os = "macos"))]
pub fn run_installed_direct_boot_product(
    _install_root: &Path,
    _state_root: &Path,
    _product_trust_root: &CurlProductReleaseTrustRoot,
    _guest_trust_root: &NativeShadowUpdateTrustRoot,
) -> Result<(), InstalledProductLifecycleError> {
    Err(InstalledProductLifecycleError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn health_query_requires_both_closed_local_probes() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = thread::spawn(move || {
            for expected in ["/live", "/ready"] {
                let (mut stream, _) = listener.accept().expect("probe connection");
                let mut request = [0_u8; 2048];
                let read = stream.read(&mut request).expect("request");
                let request = String::from_utf8_lossy(&request[..read]);
                assert!(request.starts_with(&format!("GET {expected} HTTP/1.1")));
                let (probe, field) = if expected == "/live" {
                    ("live", "\"live\":true")
                } else {
                    ("ready", "\"ready\":true")
                };
                let body = format!(
                    "{{\"schema\":\"boole.native-shadow.service-health.v1\",\"probe\":\"{probe}\",{field},\"loopbackOnly\":true,\"mineableNow\":false,\"activationAllowed\":false}}"
                );
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .expect("response");
            }
        });

        let status = query_installed_direct_boot_health_at(
            &format!("http://{address}"),
            std::time::Duration::from_secs(2),
        )
        .expect("healthy service");
        assert_eq!(status["live"]["live"], true);
        assert_eq!(status["ready"]["ready"], true);
        server.join().expect("server thread");
    }
}
