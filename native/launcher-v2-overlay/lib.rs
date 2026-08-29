//! Privileged native-shadow launcher boundary, successor source generation.
//!
//! This complete replacement is applied only in the temporary launcher-v2
//! export.  The tracked v1 crate remains byte-for-byte available for the image
//! and build records that already name it.

#[cfg(all(
    target_os = "linux",
    feature = "linux-arm64-authority",
    not(target_arch = "aarch64")
))]
compile_error!("linux-arm64-authority requires aarch64 Linux");

#[cfg(all(
    target_os = "linux",
    not(feature = "linux-arm64-authority"),
    not(target_arch = "x86_64")
))]
compile_error!("the default native-shadow authority requires x86_64 Linux");

pub mod active_execution;
mod authority_arch;
mod cgroupfs_fd;
pub mod closed_local_replay_startup;
#[cfg(any(target_os = "linux", test))]
pub mod console_evidence;
#[cfg(any(target_os = "linux", test))]
mod drop_privilege_snapshot;
pub mod instance_id;
pub mod lifetime_lock;
pub mod manager_cgroup;
#[allow(dead_code)]
pub mod per_request_containment;
pub mod privilege;
pub mod qualification;
pub mod readiness;
pub mod runtime_rootfs_replay;
pub mod startup;
pub mod startup_recovery;
pub mod toolchain_compatibility;
