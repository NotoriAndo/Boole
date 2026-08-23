//! Privileged native-shadow launcher boundary.
//!
//! The current slice contains the disabled, request-free qualification core,
//! a Linux adapter for one already-connected Unix stream, and the fixed
//! launcher root/capability prerequisite check. It does not bind or accept a
//! socket, spawn a checker, or expose an execution/report API.

pub mod privilege;
pub mod qualification;
