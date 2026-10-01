//! Boole H1 vendored crate for darkfi's halo2 gadgets (built in scratch only).
//!
//! `src/zk/gadget/*.rs` are the gadget sources of darkrenaissance/darkfi at the pin, copied verbatim under their
//! module path `zk::gadget`; the crate builds them against the halo2 fork darkfi locks (parazyd/halo2, branch
//! v050, commit 98d449b8, with the `circuit-params` feature darkfi enables) and darkfi's own Cargo.lock.  Only the
//! chips' non-test code is compiled; `boole_h1_darkfi` holds the wrappers and `boole_h1` the shared driver.

pub mod zk {
    pub mod gadget {
        pub mod arithmetic;
        pub mod cond_select;
        pub mod is_equal;
        pub mod less_than;
        pub mod native_range_check;
        pub mod small_range_check;
    }
}

pub mod boole_h1;
pub mod boole_h1_darkfi;
