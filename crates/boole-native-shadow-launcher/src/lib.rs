//! Privileged native-shadow launcher boundary.
//!
//! The current slice contains the disabled, request-free qualification core,
//! a Linux adapter for one already-connected Unix stream, and the fixed
//! launcher root/capability prerequisite check, the input-free pre-lock
//! composition of that proof with installed authority and fixed service
//! identities, the fixed-path lifetime launcher lock, and a one-shot launcher
//! instance identity. It does not bind or accept a socket, recover cgroups,
//! spawn a checker, or expose an execution/report API.

pub mod instance_id;
pub mod lifetime_lock;
pub mod privilege;
pub mod qualification;
pub mod startup;
