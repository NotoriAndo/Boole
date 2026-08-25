//! Shared vocabulary of the direct-Linux boot inputs.
//!
//! The update contract, the host controller and the canary must never invent
//! separate spellings for these files: a digest can bind bytes only after all
//! three boundaries agree which role those bytes serve.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GuestBootArtifactRole {
    GuestKernel,
    GuestInitrd,
    GuestRootDisk,
}

impl GuestBootArtifactRole {
    pub const ALL: [Self; 3] = [Self::GuestKernel, Self::GuestInitrd, Self::GuestRootDisk];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GuestKernel => "guest-kernel",
            Self::GuestInitrd => "guest-initrd",
            Self::GuestRootDisk => "guest-root-disk",
        }
    }
}

impl fmt::Display for GuestBootArtifactRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
