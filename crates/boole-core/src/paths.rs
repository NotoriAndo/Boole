//! P2.3 — centralized resolver for the on-disk Boole home directory.
//!
//! Historically each CLI subsystem (`keys_dir`, `sessions_dir`,
//! `signer_nonces_dir`) duplicated the same `$HOME/.boole/<subdir>`
//! fallback, which made it impossible for operators to relocate Boole
//! state with a single env var. `BOOLE_HOME` now lets ops point every
//! subsystem at one root (e.g., `/var/lib/boole`); per-subdir overrides
//! (`BOOLE_KEYS_DIR` etc.) still win when set, preserving every
//! existing integration test contract.
//!
//! The pure `boole_home_root_from` form takes an `env` reader so unit
//! tests can exercise every branch without mutating process-global env.
//! The thin `boole_home_root` wrapper reads the real `std::env`.
//!
//! Path-only resolution; no I/O, no directory creation.

use std::path::PathBuf;

/// Default leaf under `$HOME` when neither `BOOLE_HOME` nor a per-subdir
/// override is set. Kept as a module-level constant so downstream
/// migration tooling can detect the legacy layout without re-deriving
/// the literal.
pub const DEFAULT_HOME_LEAF: &str = ".boole";

/// Last-resort root when neither `BOOLE_HOME` nor `HOME` is set. Better
/// to write to a known relative location and surface the path than to
/// crash; the existing CLI resolvers used the same fallback.
const FALLBACK_HOME_BASE: &str = ".";

/// Resolve the Boole home directory from an explicit env reader. Pure
/// — no process env access — so tests can cover every branch in
/// parallel without serializing on `std::env::set_var`.
pub fn boole_home_root_from<F>(env: F) -> PathBuf
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(explicit) = env("BOOLE_HOME") {
        return PathBuf::from(explicit);
    }
    let base = env("HOME").unwrap_or_else(|| FALLBACK_HOME_BASE.to_string());
    PathBuf::from(base).join(DEFAULT_HOME_LEAF)
}

/// Process-env wrapper around [`boole_home_root_from`]. Reads
/// `BOOLE_HOME` and `HOME` from `std::env`.
pub fn boole_home_root() -> PathBuf {
    boole_home_root_from(|k| std::env::var(k).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_fn<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(name, _)| *name == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn boole_home_returns_explicit_root_when_boole_home_set() {
        let env = env_fn(&[("BOOLE_HOME", "/var/lib/boole"), ("HOME", "/home/alice")]);
        assert_eq!(boole_home_root_from(env), PathBuf::from("/var/lib/boole"));
    }

    #[test]
    fn boole_home_falls_back_to_home_dotboole_when_boole_home_unset() {
        let env = env_fn(&[("HOME", "/home/alice")]);
        assert_eq!(
            boole_home_root_from(env),
            PathBuf::from("/home/alice/.boole")
        );
    }

    #[test]
    fn boole_home_uses_dot_when_both_envs_unset() {
        let env = env_fn(&[]);
        assert_eq!(boole_home_root_from(env), PathBuf::from("./.boole"));
    }

    #[test]
    fn boole_home_prefers_boole_home_over_home_even_if_home_set() {
        let env = env_fn(&[("HOME", "/home/alice"), ("BOOLE_HOME", "/srv/boole")]);
        assert_eq!(boole_home_root_from(env), PathBuf::from("/srv/boole"));
    }
}
