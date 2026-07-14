//! SC.9b / ADR-0016 (a)(a-2) — named-network checker pin + executable
//! toolchain identity enforcement at boot.
//!
//! `checker_artifact_hash` commits the checker SOURCES and the
//! `lean-toolchain` pin text, but the checker launches `lean`/`lake` from
//! the process environment: a source-hash match with a different
//! executable toolchain would still judge proofs under an identity the
//! network never pinned. A named network whose compiled preset pins the
//! checker therefore refuses to boot unless all three agree:
//!
//!   1. the configured checker directory hashes to the pinned
//!      `checker_artifact_hash`;
//!   2. the release-channel manifest (`RELEASE-MANIFEST.json`, covered by
//!      `SHA256SUMS` + git tag — the P3.6-subset minimal channel) declares
//!      exactly that pin;
//!   3. the toolchain the checker process would ACTUALLY execute
//!      (`lake env lean` resolved from the package dir, elan-dispatched by
//!      its `lean-toolchain`) matches the manifest's declared Lean
//!      version + githash and Lake version.
//!
//! Mismatch anywhere is a typed boot refusal, not a warning (ADR-0016
//! (a-2): "A source-directory hash match with a different executable
//! toolchain is a typed boot refusal").

use std::path::Path;

use anyhow::Context;

/// Enforce the pinned checker artifact + executable toolchain identity for
/// a named network. Called at boot BEFORE the genesis-hash gate so the
/// refusal names the most specific divergence.
pub(crate) fn enforce_pinned_checker_toolchain(
    network_id: &str,
    pinned_artifact_hash: &str,
    checker_dir: &Path,
) -> anyhow::Result<()> {
    let actual = boole_lean_runner::checker_artifact_hash(checker_dir).with_context(|| {
        format!(
            "network {network_id} pins its checker; computing the artifact hash of {} failed",
            checker_dir.display()
        )
    })?;
    if actual != pinned_artifact_hash {
        anyhow::bail!(
            "network {network_id} pins checker_artifact_hash {pinned_artifact_hash}, but the \
             configured checker at {} hashes to {actual} — refusing to boot with an unpinned \
             checker (SC.9b / ADR-0016 (a))",
            checker_dir.display()
        );
    }

    let manifest_path = checker_dir.join("RELEASE-MANIFEST.json");
    let manifest_text = std::fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "network {network_id} pins its checker; the release-channel manifest {} is required \
             (SC.9b — tag + SHA256SUMS minimal channel)",
            manifest_path.display()
        )
    })?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .with_context(|| format!("parse {}", manifest_path.display()))?;
    let field = |key: &str| -> anyhow::Result<&str> {
        manifest[key].as_str().ok_or_else(|| {
            anyhow::anyhow!(
                "release manifest {} is missing string field {key}",
                manifest_path.display()
            )
        })
    };
    let schema = field("schema")?;
    if schema != "boole.checker.release.v1" {
        anyhow::bail!(
            "release manifest {} has unsupported schema {schema}",
            manifest_path.display()
        );
    }
    let manifest_hash = field("checkerArtifactHash")?;
    if manifest_hash != pinned_artifact_hash {
        anyhow::bail!(
            "network {network_id} pins checker_artifact_hash {pinned_artifact_hash}, but the \
             release manifest {} declares {manifest_hash} — refusing to boot from a release \
             channel that disagrees with the network pin (SC.9b)",
            manifest_path.display()
        );
    }

    let expected_lean_version = field("leanVersion")?.to_string();
    let expected_lean_githash = field("leanGithash")?.to_string();
    let expected_lake_version = field("lakeVersion")?.to_string();
    let toolchain =
        boole_lean_runner::effective_toolchain_identity(checker_dir).with_context(|| {
            format!(
                "network {network_id} pins its checker; probing the effective toolchain of {} \
                 failed",
                checker_dir.display()
            )
        })?;
    if toolchain.lean_githash != expected_lean_githash {
        anyhow::bail!(
            "network {network_id} released its checker against lean githash \
             {expected_lean_githash}, but the checker at {} would execute lean githash {} \
             ({}) — refusing to boot with a different executable toolchain (SC.9b / ADR-0016 \
             (a-2))",
            checker_dir.display(),
            toolchain.lean_githash,
            toolchain.lean_version
        );
    }
    let actual_lean_version = toolchain.lean_version_token().unwrap_or_default();
    if actual_lean_version != expected_lean_version {
        anyhow::bail!(
            "network {network_id} released its checker against lean version \
             {expected_lean_version}, but the checker at {} would execute lean version \
             {actual_lean_version} — refusing to boot with a different executable toolchain \
             (SC.9b / ADR-0016 (a-2))",
            checker_dir.display()
        );
    }
    let actual_lake_version = toolchain.lake_version_token().unwrap_or_default();
    if actual_lake_version != expected_lake_version {
        anyhow::bail!(
            "network {network_id} released its checker against lake version \
             {expected_lake_version}, but the checker at {} would execute lake version \
             {actual_lake_version} — refusing to boot with a different executable toolchain \
             (SC.9b / ADR-0016 (a-2))",
            checker_dir.display()
        );
    }
    Ok(())
}
