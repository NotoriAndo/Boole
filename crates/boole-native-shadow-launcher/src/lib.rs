//! Privileged native-shadow launcher boundary.
//!
//! The current slice contains only the disabled, request-free qualification
//! handshake core. It does not bind a socket, spawn a checker or expose an
//! execution/report API.

pub mod qualification;
