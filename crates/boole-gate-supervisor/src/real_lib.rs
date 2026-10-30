//! Versioned real gate extension; the historical source release is compiled unchanged.
#[path = "lib.rs"]
mod legacy;
pub use legacy::*;
#[cfg(test)]
extern crate self as boole_gate_supervisor;
pub mod real_admission;
pub mod real_artifact;
pub mod real_cache;
pub mod real_controller;
pub mod real_deadline;
pub mod real_decoder;
pub mod real_descriptor;
pub mod real_front;
pub mod real_json;
pub mod real_linux;
pub mod real_package;
pub mod real_process;
pub mod real_rebuild;
pub mod real_reply;
pub mod real_retry;
pub mod real_supervisor;
pub mod real_worker;

#[cfg(feature = "w0-fixture")]
pub mod real_w0;

#[cfg(all(test, unix, feature = "w0-fixture"))]
#[path = "w0_fixture.rs"]
mod w0_fixture_contract;
