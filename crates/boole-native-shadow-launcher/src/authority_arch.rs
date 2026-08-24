//! Compile-time architecture projection for the fixed Linux checker authority.
//!
//! The default remains the frozen x86_64 authority.  The arm64 successor is
//! opt-in and never consults an environment variable, request, or runtime host
//! value to select paths.

#[cfg(feature = "linux-arm64-authority")]
pub(crate) const EXPECTED_RUST_HOST: &str = "aarch64-unknown-linux-gnu";
#[cfg(not(feature = "linux-arm64-authority"))]
pub(crate) const EXPECTED_RUST_HOST: &str = "x86_64-unknown-linux-gnu";

#[cfg(all(feature = "linux-arm64-authority", any(target_os = "linux", test)))]
pub(crate) const LANDLOCK_EXECUTE_DIRECTORIES: &[&str] = &[
    "/lib",
    "/usr/bin",
    "/usr/lib",
    "/opt/boole/native-checker-toolchain",
    "/work",
];
#[cfg(all(not(feature = "linux-arm64-authority"), any(target_os = "linux", test)))]
pub(crate) const LANDLOCK_EXECUTE_DIRECTORIES: &[&str] = &[
    "/lib",
    "/lib64",
    "/usr/bin",
    "/usr/lib",
    "/opt/boole/native-checker-toolchain",
    "/work",
];

#[cfg(all(feature = "linux-arm64-authority", any(target_os = "linux", test)))]
pub(crate) const GCC_FRONTEND_PATH: &str = "/usr/libexec/gcc/aarch64-linux-gnu/13/cc1";
#[cfg(all(not(feature = "linux-arm64-authority"), any(target_os = "linux", test)))]
pub(crate) const GCC_FRONTEND_PATH: &str = "/usr/libexec/gcc/x86_64-linux-gnu/13/cc1";

#[cfg(all(feature = "linux-arm64-authority", any(target_os = "linux", test)))]
pub(crate) const GCC_LINKER_PATH: &str = "/usr/libexec/gcc/aarch64-linux-gnu/13/collect2";
#[cfg(all(not(feature = "linux-arm64-authority"), any(target_os = "linux", test)))]
pub(crate) const GCC_LINKER_PATH: &str = "/usr/libexec/gcc/x86_64-linux-gnu/13/collect2";

#[cfg(all(feature = "linux-arm64-authority", any(target_os = "linux", test)))]
pub(crate) const RUNTIME_ROOTFS_CONTENT_MANIFEST_SHA256: &str =
    "200f025756d4c83e15a306feac982a91aa6130979665d0265c33aee95f3987aa";
#[cfg(all(not(feature = "linux-arm64-authority"), any(target_os = "linux", test)))]
pub(crate) const RUNTIME_ROOTFS_CONTENT_MANIFEST_SHA256: &str =
    "957761ceaeca18e0af516ed200c7587aa57a609b16ebfe63dacb1371df489763";

#[cfg(all(feature = "linux-arm64-authority", any(target_os = "linux", test)))]
pub(crate) const RUNTIME_ROOTFS_CONTENT_MANIFEST_SIZE: u64 = 1_285_116;
#[cfg(all(not(feature = "linux-arm64-authority"), any(target_os = "linux", test)))]
pub(crate) const RUNTIME_ROOTFS_CONTENT_MANIFEST_SIZE: u64 = 1_275_874;

#[cfg(all(feature = "linux-arm64-authority", any(target_os = "linux", test)))]
pub(crate) const RUNTIME_ROOTFS_CONTENT_MANIFEST_SCHEMA: &str =
    "boole.native-shadow.rootfs-content-manifest.arm64.v1";
#[cfg(all(not(feature = "linux-arm64-authority"), any(target_os = "linux", test)))]
pub(crate) const RUNTIME_ROOTFS_CONTENT_MANIFEST_SCHEMA: &str =
    "boole.native-shadow.rootfs-content-manifest.v1";

#[cfg(test)]
mod tests {
    use super::{
        EXPECTED_RUST_HOST, GCC_FRONTEND_PATH, GCC_LINKER_PATH, LANDLOCK_EXECUTE_DIRECTORIES,
        RUNTIME_ROOTFS_CONTENT_MANIFEST_SCHEMA, RUNTIME_ROOTFS_CONTENT_MANIFEST_SHA256,
        RUNTIME_ROOTFS_CONTENT_MANIFEST_SIZE,
    };

    #[cfg(feature = "linux-arm64-authority")]
    #[test]
    fn arm64_authority_uses_only_arm64_host_and_gcc_paths() {
        assert_eq!(EXPECTED_RUST_HOST, "aarch64-unknown-linux-gnu");
        assert_eq!(
            GCC_FRONTEND_PATH,
            "/usr/libexec/gcc/aarch64-linux-gnu/13/cc1"
        );
        assert_eq!(
            GCC_LINKER_PATH,
            "/usr/libexec/gcc/aarch64-linux-gnu/13/collect2"
        );
        assert!(!LANDLOCK_EXECUTE_DIRECTORIES.contains(&"/lib64"));
        assert_eq!(
            RUNTIME_ROOTFS_CONTENT_MANIFEST_SHA256,
            "200f025756d4c83e15a306feac982a91aa6130979665d0265c33aee95f3987aa"
        );
        assert_eq!(RUNTIME_ROOTFS_CONTENT_MANIFEST_SIZE, 1_285_116);
        assert_eq!(
            RUNTIME_ROOTFS_CONTENT_MANIFEST_SCHEMA,
            "boole.native-shadow.rootfs-content-manifest.arm64.v1"
        );
    }

    #[cfg(not(feature = "linux-arm64-authority"))]
    #[test]
    fn default_authority_keeps_the_frozen_x86_paths() {
        assert_eq!(EXPECTED_RUST_HOST, "x86_64-unknown-linux-gnu");
        assert_eq!(
            GCC_FRONTEND_PATH,
            "/usr/libexec/gcc/x86_64-linux-gnu/13/cc1"
        );
        assert_eq!(
            GCC_LINKER_PATH,
            "/usr/libexec/gcc/x86_64-linux-gnu/13/collect2"
        );
        assert!(LANDLOCK_EXECUTE_DIRECTORIES.contains(&"/lib64"));
        assert_eq!(
            RUNTIME_ROOTFS_CONTENT_MANIFEST_SHA256,
            "957761ceaeca18e0af516ed200c7587aa57a609b16ebfe63dacb1371df489763"
        );
        assert_eq!(RUNTIME_ROOTFS_CONTENT_MANIFEST_SIZE, 1_275_874);
        assert_eq!(
            RUNTIME_ROOTFS_CONTENT_MANIFEST_SCHEMA,
            "boole.native-shadow.rootfs-content-manifest.v1"
        );
    }
}
