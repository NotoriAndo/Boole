//! Privileged native-shadow launcher boundary.
//!
//! The current slice contains the disabled, request-free qualification core,
//! a Linux adapter for one already-connected Unix stream, and the fixed
//! launcher root/capability prerequisite check, the input-free pre-lock
//! composition of that proof with installed authority and fixed service
//! identities, the fixed-path lifetime launcher lock, and a one-shot launcher
//! instance identity. It also exposes the next startup boundary for entering
//! the fixed manager cgroup, removing exact startup `run-*` orphan leaves,
//! running the four fixed pre-bind toolchain compatibility probes and consuming
//! that complete proof chain into the only token allowed to serve request-free
//! qualification. A staged active-execution core can exercise the strict
//! successor wire sequence in-module, but it deliberately exposes no listener
//! or executor construction path. It also contains the crate-private,
//! fixed-checker per-request Linux containment core. Until that core, a verified
//! runtime-rootfs replay token, and the separate exact fixed-case replay grant
//! are all consumed together, production code still cannot bind a socket, spawn
//! a checker, or serve an execution/report API. Every installed policy and
//! readiness frame remains `activationAllowed=false`.

pub mod active_execution;
mod cgroupfs_fd;
pub mod instance_id;
pub mod lifetime_lock;
pub mod manager_cgroup;
#[allow(dead_code)] // Wired by the separately reviewed active-execution slice.
pub mod per_request_containment;
pub mod privilege;
pub mod qualification;
pub mod readiness;
pub mod startup;
pub mod startup_recovery;
pub mod toolchain_compatibility;
