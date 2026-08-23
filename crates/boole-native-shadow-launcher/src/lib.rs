//! Privileged native-shadow launcher boundary.
//!
//! The current slice contains the disabled, request-free qualification core
//! and a Linux adapter for one already-connected Unix stream. It does not bind
//! or accept a socket, spawn a checker, or expose an execution/report API.

pub mod qualification;
