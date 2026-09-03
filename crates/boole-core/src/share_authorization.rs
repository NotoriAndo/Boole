//! SC.1 (ADR-0015 (b)/(b-1)) — envelope-intrinsic verification of the
//! submitter-signed `boole.signer.work.v2` authorization carried in
//! `SelectedShareEvidence.signed_work`.
//!
//! "Envelope-intrinsic" means: everything checkable from the envelope
//! bytes alone — schema shape, the ed25519 signature over the canonical
//! payload (honoring the envelope's own `network_id` binding), and the
//! internal `requestHash == SHA-256(canonical(workPayload))` link. It
//! deliberately excludes node-local session policy (registry lookup,
//! nonce ledger, fee ceilings): a replaying peer never sees the
//! submitter's session registry, so consensus verification MUST NOT
//! depend on it. Replay (`replay_evidence`), gossip ingress, and the
//! offline receipt audit all funnel through this one function — one
//! verification policy, no second copy.

use serde_json::Value;

use crate::block::ShareWorkAuthorization;
use crate::signed_envelope::{
    canonical_payload_hash_hex, verify_signature_with_network, SIGNED_ENVELOPE_SCHEMA,
};

/// The inner payload schema of a signed submit-work envelope. The outer
/// transport shell stays `boole.signed.v1` (`SIGNED_ENVELOPE_SCHEMA`).
pub const SIGNER_WORK_V2_SCHEMA: &str = "boole.signer.work.v2";

/// The route a submit-work envelope authorizes. Bound into the signed
/// payload so a signature produced for another surface cannot be
/// replayed against `/submit`.
pub const SIGNER_WORK_ROUTE: &str = "/submit";

/// The first named network whose persisted share evidence requires a
/// network-scoped submitter authorization. Other named networks keep their
/// current optional-authorization posture; when they do carry an
/// authorization, it must still name the replayed network exactly.
pub const AUTHORIZATION_REQUIRED_NETWORK_ID: &str = "boole-testnet-2";

/// Fields extracted from a verified authorization envelope. All values
/// are attested by the submitter's signature; callers compare them
/// against the committed share/block fields they are authorizing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedShareAuthorization {
    /// The envelope signer (outer `pk`) — the session identity.
    pub signer_pk: String,
    /// The reward destination the signer authorized.
    pub reward_recipient: String,
    /// `workPayload.pk` — the mining identity the envelope covers.
    pub work_pk: String,
    /// `workPayload.n` / `workPayload.j` / `workPayload.c` — the share
    /// coordinates the envelope covers.
    pub work_n: String,
    pub work_j: String,
    pub work_c: String,
    /// `workPayload.bytes` — the exact submitted proof package (hex).
    pub work_bytes_hex: String,
    /// The network the signature is scoped to (`None` = legacy digest).
    pub network_id: Option<String>,
}

fn payload_str<'a>(payload: &'a Value, field: &str) -> anyhow::Result<&'a str> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("signedWork payload.{field} missing or not a string"))
}

fn work_payload_str<'a>(work_payload: &'a Value, field: &str) -> anyhow::Result<&'a str> {
    work_payload
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            anyhow::anyhow!("signedWork payload.workPayload.{field} missing or not a string")
        })
}

/// Verify everything the envelope itself attests and extract the
/// authorized identities. Returns an error naming the first violated
/// property; a caller maps it into its own rejection surface.
pub fn verify_share_work_authorization(
    auth: &ShareWorkAuthorization,
) -> anyhow::Result<VerifiedShareAuthorization> {
    if auth.schema != SIGNED_ENVELOPE_SCHEMA {
        anyhow::bail!(
            "signedWork schema mismatch: got {}, expected {}",
            auth.schema,
            SIGNED_ENVELOPE_SCHEMA
        );
    }
    let payload = &auth.payload;
    if !payload.is_object() {
        anyhow::bail!("signedWork payload must be an object");
    }
    let payload_schema = payload_str(payload, "schema")?;
    if payload_schema != SIGNER_WORK_V2_SCHEMA {
        anyhow::bail!(
            "signedWork payload schema mismatch: got {}, expected {}",
            payload_schema,
            SIGNER_WORK_V2_SCHEMA
        );
    }
    let route = payload_str(payload, "route")?;
    if route != SIGNER_WORK_ROUTE {
        anyhow::bail!(
            "signedWork payload route mismatch: got {}, expected {}",
            route,
            SIGNER_WORK_ROUTE
        );
    }
    let work_payload = payload
        .get("workPayload")
        .ok_or_else(|| anyhow::anyhow!("signedWork payload.workPayload missing"))?;
    if !work_payload.is_object() {
        anyhow::bail!("signedWork payload.workPayload must be an object");
    }
    let request_hash = payload_str(payload, "requestHash")?;
    let computed_request_hash = canonical_payload_hash_hex(work_payload);
    if request_hash != computed_request_hash {
        anyhow::bail!(
            "signedWork requestHash does not commit the workPayload: got {}, expected {}",
            request_hash,
            computed_request_hash
        );
    }
    let reward_recipient = payload_str(payload, "rewardRecipient")?;
    crate::Hex32::from_hex(reward_recipient).map_err(|err| {
        anyhow::anyhow!("signedWork rewardRecipient is not well-formed hex32: {err}")
    })?;

    match verify_signature_with_network(
        &auth.pk,
        &auth.signature,
        payload,
        auth.network_id.as_deref(),
    ) {
        Ok(true) => {}
        Ok(false) => anyhow::bail!("signedWork signature invalid"),
        Err(err) => anyhow::bail!("signedWork signature verification failed: {err}"),
    }

    // Admission compares proof bytes after decoding them, so uppercase and
    // lowercase hex are the same submitted work. Preserve that equivalence
    // at selection/replay too instead of stranding an already-admitted share
    // behind a raw string comparison.
    let work_bytes_hex = hex::decode(work_payload_str(work_payload, "bytes")?)
        .map(hex::encode)
        .map_err(|err| anyhow::anyhow!("signedWork workPayload.bytes is not valid hex: {err}"))?;

    Ok(VerifiedShareAuthorization {
        signer_pk: auth.pk.clone(),
        reward_recipient: reward_recipient.to_string(),
        work_pk: work_payload_str(work_payload, "pk")?.to_string(),
        work_n: work_payload_str(work_payload, "n")?.to_string(),
        work_j: work_payload_str(work_payload, "j")?.to_string(),
        work_c: work_payload_str(work_payload, "c")?.to_string(),
        work_bytes_hex,
        network_id: auth.network_id.clone(),
    })
}

/// Apply the replay network's authorization policy around the intrinsic
/// envelope verifier.
///
/// `expected_network_id: None` is the legacy replay posture: authorization
/// remains optional and a present envelope is verified using its own scope.
/// A named-network replay requires any present envelope to be scoped to that
/// exact network. `boole-testnet-2` additionally requires the envelope to be
/// present at all.
pub fn verify_share_work_authorization_for_network(
    auth: Option<&ShareWorkAuthorization>,
    expected_network_id: Option<&str>,
) -> anyhow::Result<Option<VerifiedShareAuthorization>> {
    let Some(auth) = auth else {
        if expected_network_id == Some(AUTHORIZATION_REQUIRED_NETWORK_ID) {
            anyhow::bail!(
                "signedWork authorization is required on network {}",
                AUTHORIZATION_REQUIRED_NETWORK_ID
            );
        }
        return Ok(None);
    };

    let verified = verify_share_work_authorization(auth)?;
    if let Some(expected_network_id) = expected_network_id {
        if verified.network_id.as_deref() != Some(expected_network_id) {
            anyhow::bail!(
                "signedWork network mismatch: got {}, expected {}",
                verified
                    .network_id
                    .as_deref()
                    .unwrap_or("<legacy-unscoped>"),
                expected_network_id
            );
        }
    }
    Ok(Some(verified))
}
