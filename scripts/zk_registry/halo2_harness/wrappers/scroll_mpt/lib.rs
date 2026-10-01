//! Boole H1 runner crate for scroll-tech/mpt-circuit (built in scratch only): depends on the pinned checkout by
//! path (its default features, as its tests build it) with the repository's Cargo.lock and the halo2 fork it locks
//! (scroll-tech/halo2 v1.0 at 2f5ee104, instrumented).

pub mod boole_h1;
pub mod boole_h1_mpt;
