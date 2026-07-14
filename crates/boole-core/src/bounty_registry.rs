use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

const HEX32_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BountyVerifier {
    pub kind: String,
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounty {
    pub id: String,
    pub domain: String,
    pub problem_hash: String,
    pub verifier: BountyVerifier,
    pub reward: String,
    pub deadline: u64,
    pub status: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateBountyInput {
    pub id: String,
    pub domain: String,
    pub problem_hash: String,
    pub verifier_kind: String,
    pub verifier_metadata: Map<String, Value>,
    pub reward: u128,
    pub deadline: u64,
    pub ts: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateStatusInput {
    pub id: String,
    pub status: String,
    pub ts: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitProofInput {
    pub bounty_id: String,
    pub proof_hash: String,
    pub prover: String,
    pub accepted: bool,
    pub ts: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitProofResult {
    pub accepted: bool,
    pub duplicate: bool,
    pub bounty: Bounty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
enum BountyEvent {
    #[serde(rename = "create")]
    Create { bounty: Bounty },
    #[serde(rename = "status")]
    Status { id: String, status: String, ts: u64 },
    #[serde(rename = "proof")]
    Proof {
        #[serde(rename = "bountyId")]
        bounty_id: String,
        #[serde(rename = "proofHash")]
        proof_hash: String,
        prover: String,
        accepted: bool,
        ts: u64,
    },
}

/// On-disk envelope for the static `/bounties` catalog: `{ version: 1,
/// bounties: [...] }`. Mirrors `WorkManifestList` so the read surface
/// loader can ship without pulling in the registry's mutation API. When
/// S12 lands the announce flow, the catalog is replayed through
/// `BountyRegistry::apply_event_fixture` `create` events instead.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BountyList {
    pub version: u32,
    pub bounties: Vec<Bounty>,
}

/// Validate a decoded `BountyList` envelope and return the inner bounty vec.
///
/// Runtime crates own file IO. Core owns the schema contract: format bumps must
/// explicitly rev `version`, and callers should never see a silent shape drift.
pub fn bounties_from_list(list: BountyList) -> anyhow::Result<Vec<Bounty>> {
    if list.version != 1 {
        anyhow::bail!(
            "unsupported bounty list version {}: expected 1",
            list.version
        );
    }
    Ok(list.bounties)
}

#[derive(Debug, Default, Clone)]
pub struct BountyRegistry {
    bounties: HashMap<String, Bounty>,
    order: Vec<String>,
    proofs: HashMap<String, HashMap<String, bool>>,
}

impl BountyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&mut self, input: CreateBountyInput) -> Result<Bounty, String> {
        validate_create(&input)?;
        if self.bounties.contains_key(&input.id) {
            return Err(format!("bounty id already exists: {}", input.id));
        }
        let bounty = bounty_from_input(input);
        self.order.push(bounty.id.clone());
        self.bounties.insert(bounty.id.clone(), bounty.clone());
        Ok(bounty)
    }

    pub fn update_status(&mut self, input: UpdateStatusInput) -> Result<Bounty, String> {
        let existing = self
            .bounties
            .get(&input.id)
            .cloned()
            .ok_or_else(|| format!("unknown bounty id: {}", input.id))?;
        validate_status_transition(&existing.status, &input.status)?;
        let updated = Bounty {
            status: input.status,
            updated_at: input.ts,
            ..existing
        };
        self.bounties.insert(updated.id.clone(), updated.clone());
        Ok(updated)
    }

    pub fn submit_proof(&mut self, input: SubmitProofInput) -> Result<SubmitProofResult, String> {
        if !is_hex32(&input.proof_hash) {
            return Err("submitProof proofHash must be 32-byte lowercase hex".to_string());
        }
        if !is_hex32(&input.prover) {
            return Err("submitProof prover must be 32-byte lowercase hex".to_string());
        }
        let bounty = self
            .bounties
            .get(&input.bounty_id)
            .cloned()
            .ok_or_else(|| format!("unknown bounty id: {}", input.bounty_id))?;

        if let Some(accepted) = self
            .proofs
            .get(&input.bounty_id)
            .and_then(|seen| seen.get(&input.proof_hash))
            .copied()
        {
            return Ok(SubmitProofResult {
                accepted,
                duplicate: true,
                bounty,
            });
        }

        if bounty.status != "open" {
            return Err(format!(
                "cannot submit proof to terminal bounty {}: {}",
                input.bounty_id, bounty.status
            ));
        }

        self.record_proof(&input.bounty_id, &input.proof_hash, input.accepted);
        let mut updated = bounty.clone();
        if input.accepted {
            updated.status = "solved".to_string();
            updated.updated_at = input.ts;
            self.bounties
                .insert(input.bounty_id.clone(), updated.clone());
        }
        Ok(SubmitProofResult {
            accepted: input.accepted,
            duplicate: false,
            bounty: updated,
        })
    }

    pub fn has_proof(&self, bounty_id: &str, proof_hash: &str) -> Option<bool> {
        self.proofs
            .get(bounty_id)
            .and_then(|seen| seen.get(proof_hash))
            .copied()
    }

    pub fn get(&self, id: &str) -> Option<Bounty> {
        self.bounties.get(id).cloned()
    }

    pub fn list(&self) -> Vec<Bounty> {
        self.order
            .iter()
            .filter_map(|id| self.bounties.get(id).cloned())
            .collect()
    }

    pub fn list_open(&self) -> Vec<Bounty> {
        let mut open: Vec<_> = self
            .list()
            .into_iter()
            .filter(|bounty| bounty.status == "open")
            .collect();
        open.sort_by_key(|bounty| bounty.deadline);
        open
    }

    pub fn size(&self) -> usize {
        self.bounties.len()
    }

    pub fn apply_event_fixture(&mut self, value: Value) -> Result<(), String> {
        let event: BountyEvent = serde_json::from_value(value).map_err(|err| err.to_string())?;
        self.apply_event(event)
    }

    fn apply_event(&mut self, event: BountyEvent) -> Result<(), String> {
        match event {
            BountyEvent::Create { bounty } => {
                if self.bounties.contains_key(&bounty.id) {
                    return Err(format!("duplicates id {}", bounty.id));
                }
                self.order.push(bounty.id.clone());
                self.bounties.insert(bounty.id.clone(), bounty);
                Ok(())
            }
            BountyEvent::Status { id, status, ts } => {
                let existing = self
                    .bounties
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| format!("updates unknown id {id}"))?;
                validate_status_transition(&existing.status, &status)?;
                self.bounties.insert(
                    id,
                    Bounty {
                        status,
                        updated_at: ts,
                        ..existing
                    },
                );
                Ok(())
            }
            BountyEvent::Proof {
                bounty_id,
                proof_hash,
                accepted,
                ts,
                ..
            } => {
                let bounty = self
                    .bounties
                    .get(&bounty_id)
                    .cloned()
                    .ok_or_else(|| format!("proof references unknown bounty {bounty_id}"))?;
                self.record_proof(&bounty_id, &proof_hash, accepted);
                if accepted && bounty.status == "open" {
                    self.bounties.insert(
                        bounty_id,
                        Bounty {
                            status: "solved".to_string(),
                            updated_at: ts,
                            ..bounty
                        },
                    );
                }
                Ok(())
            }
        }
    }

    fn record_proof(&mut self, bounty_id: &str, proof_hash: &str, accepted: bool) {
        self.proofs
            .entry(bounty_id.to_string())
            .or_default()
            .insert(proof_hash.to_string(), accepted);
    }
}

fn bounty_from_input(input: CreateBountyInput) -> Bounty {
    Bounty {
        id: input.id,
        domain: input.domain,
        problem_hash: input.problem_hash,
        verifier: BountyVerifier {
            kind: input.verifier_kind,
            metadata: input.verifier_metadata,
        },
        reward: input.reward.to_string(),
        deadline: input.deadline,
        status: "open".to_string(),
        created_at: input.ts,
        updated_at: input.ts,
    }
}

fn validate_create(input: &CreateBountyInput) -> Result<(), String> {
    if !is_valid_id(&input.id) {
        return Err("bounty id must be 1-128 printable ASCII chars without whitespace".to_string());
    }
    if !is_valid_domain(&input.domain) {
        return Err(format!(
            "bounty domain must be 1-{MAX_DOMAIN_LEN} chars of dot-separated \
             lowercase tokens ([a-z0-9] with interior hyphens); got {:?}",
            input.domain
        ));
    }
    if !is_hex32(&input.problem_hash) {
        return Err("bounty problemHash must be 32-byte lowercase hex".to_string());
    }
    if input.verifier_kind.is_empty() {
        return Err("bounty verifier.kind must be a non-empty string".to_string());
    }
    validate_verifier_metadata(&input.verifier_metadata)?;
    if input.reward == 0 {
        return Err("bounty reward must be a positive bigint".to_string());
    }
    if input.deadline == 0 {
        return Err("bounty deadline must be a positive integer (unix ms)".to_string());
    }
    Ok(())
}

const MAX_DOMAIN_LEN: usize = 64;

/// PM.3 (audit U40/U42) — `domain` doubles as `family_id` and is persisted
/// into records downstream consumers treat as "Lean-verified", so it must
/// stay a machine token, never prose: dot-separated lowercase segments,
/// each `[a-z0-9]` with interior hyphens (the shape every shipped domain
/// already uses, e.g. `lean.protocol-invariant`).
fn is_valid_domain(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > MAX_DOMAIN_LEN {
        return false;
    }
    domain.split('.').all(|segment| {
        !segment.is_empty()
            && !segment.starts_with('-')
            && !segment.ends_with('-')
            && segment
                .bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
    })
}

/// Longest legitimate metadata string is a Lean theorem `statement`;
/// anything past this cap is corpus-poisoning surface, not a statement.
const MAX_METADATA_STRING_LEN: usize = 16 * 1024;

/// PM.3 (audit U40/U42) — `verifier.metadata` is announcer-controlled and
/// stored verbatim next to verified proofs, so only keys a shipped
/// verifier actually reads are accepted, each with the type that verifier
/// expects. A new verifier that reads a new key extends this table in the
/// same change that ships the verifier.
fn validate_verifier_metadata(metadata: &Map<String, Value>) -> Result<(), String> {
    for (key, value) in metadata {
        match key.as_str() {
            "statement" | "verifierHash" | "profile" | "template" => {
                let Some(text) = value.as_str() else {
                    return Err(format!("bounty verifier.metadata.{key} must be a string"));
                };
                if text.len() > MAX_METADATA_STRING_LEN {
                    return Err(format!(
                        "bounty verifier.metadata.{key} exceeds {MAX_METADATA_STRING_LEN} bytes"
                    ));
                }
            }
            // SC.9a / ADR-0016 (b) — `maxSteps` is retired: it was never
            // read by any verifier, and a metadata field that LOOKS like a
            // step budget but binds nothing is exactly the ambiguity the
            // committed budget (family manifest `maxHeartbeats`/
            // `maxRecDepth`, base-lane rule constants) exists to remove.
            other => {
                return Err(format!(
                    "bounty verifier.metadata key {other:?} is not accepted; \
                     allowed keys: statement, verifierHash, profile, template"
                ));
            }
        }
    }
    Ok(())
}

fn validate_status_transition(current: &str, next: &str) -> Result<(), String> {
    if !matches!(next, "open" | "solved" | "expired" | "withdrawn") {
        return Err(format!("invalid status: {next}"));
    }
    if matches!(current, "solved" | "expired" | "withdrawn") {
        return Err(format!(
            "cannot transition from terminal status {current} to {next}"
        ));
    }
    Ok(())
}

fn is_valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

fn is_hex32(value: &str) -> bool {
    value.len() == HEX32_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
