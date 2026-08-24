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
//! qualification. The closed-local replay path consumes that same proof chain,
//! a verified runtime-rootfs replay token, and the exact fixed-case replay grant
//! before opening one bounded listener. Each accepted request can invoke only
//! the compiled-in native checker through the crate-private per-request Linux
//! containment core and returns one independently validated report. No caller
//! can select an executable, arguments, environment, authority bytes, or verdict.
//! The only installed replay cases remain non-issuable fixtures and every policy
//! and readiness frame remains `activationAllowed=false`; this crate therefore
//! does not activate public mining, block admission, or rewards.

pub mod active_execution;
mod cgroupfs_fd;
pub mod closed_local_replay_startup;
pub mod instance_id;
pub mod lifetime_lock;
pub mod manager_cgroup;
#[allow(dead_code)] // Wired by the separately reviewed active-execution slice.
pub mod per_request_containment;
pub mod privilege;
pub mod qualification;
pub mod readiness;
pub mod runtime_rootfs_replay;
pub mod startup;
pub mod startup_recovery;
pub mod toolchain_compatibility;
