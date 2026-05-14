use crate::Hex32;
use serde::{Deserialize, Serialize};

/// Maximum number of blocks a session can remain active without an explicit
/// owner re-issue. Bounds worst-case damage from a compromised session whose
/// revocation has not yet propagated to the node (gap G4 in the agent wallet
/// plan). Tunable; conservative default chosen so that even at ~5 s/block
/// (~17,280 blocks/day) the window stays under 24 h.
pub const MAX_SESSION_LIFETIME_BLOCKS: u64 = 17_280;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    pub session_pk: String,
    pub owner_pk: String,
    pub agent_pk: String,
    pub fixed_reward_recipient: String,
    pub allowed_family_root: String,
    pub max_fee_per_request: String,
    pub activation_height: u64,
    pub expiry_height: u64,
    pub revoked: bool,
    pub policy_hash: String,
}

/// A signature request handed to the local W3 signer before it commits to
/// signing a work or x402 payment payload. The signer authorizes the request
/// against the session's policy (route / family / verifier / fee) and against
/// invariants the protocol expects (requestHash hex32, non-empty nonce).
///
/// The type lives in `boole-core` so the node can later validate the same
/// shape when N1.x/N2.x ship; today only the local signer consumes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignerRequest {
    pub route: String,
    pub family_id: String,
    pub verifier_id: String,
    pub fee: String,
    pub request_hash: String,
    pub nonce: String,
}

impl SignerRequest {
    pub fn test_fixture() -> Self {
        let d = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_string();
        Self {
            route: "/verify-answer".to_string(),
            family_id: "boole.protocol-invariant.v01".to_string(),
            verifier_id: "lean-runner-v01".to_string(),
            fee: "10".to_string(),
            request_hash: d,
            nonce: "n-1".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPolicy {
    pub can_submit_work: bool,
    pub can_pay_verification_fee: bool,
    pub can_withdraw: bool,
    pub can_transfer: bool,
    pub allowed_routes: Vec<String>,
    pub allowed_family_ids: Vec<String>,
    pub allowed_verifier_ids: Vec<String>,
    pub max_fee_per_request: String,
    pub daily_fee_cap: String,
}

impl SessionState {
    pub fn validate_at_height(&self, height: u64) -> anyhow::Result<()> {
        validate_hex32("sessionPk", &self.session_pk)?;
        validate_hex32("ownerPk", &self.owner_pk)?;
        validate_hex32("agentPk", &self.agent_pk)?;
        validate_hex32("fixedRewardRecipient", &self.fixed_reward_recipient)?;
        validate_hex32("allowedFamilyRoot", &self.allowed_family_root)?;
        validate_hex32("policyHash", &self.policy_hash)?;
        let _ = self.max_fee_per_request.parse::<u128>()?;
        let mut role_keys = std::collections::BTreeSet::new();
        for key in [
            &self.session_pk,
            &self.owner_pk,
            &self.agent_pk,
            &self.fixed_reward_recipient,
        ] {
            if !role_keys.insert(key) {
                anyhow::bail!("session role keys must be unique");
            }
        }
        if self.revoked {
            anyhow::bail!("session revoked");
        }
        if height < self.activation_height {
            anyhow::bail!("session not active");
        }
        if height >= self.expiry_height {
            anyhow::bail!("session expired");
        }
        if self.expiry_height.saturating_sub(self.activation_height) > MAX_SESSION_LIFETIME_BLOCKS {
            anyhow::bail!("session lifetime exceeds MAX_SESSION_LIFETIME_BLOCKS");
        }
        Ok(())
    }

    pub fn test_fixture() -> Self {
        let a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
        let b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string();
        let c = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_string();
        let d = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_string();
        let e = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string();
        Self {
            session_pk: a,
            owner_pk: b,
            agent_pk: c,
            fixed_reward_recipient: d.clone(),
            allowed_family_root: e.clone(),
            max_fee_per_request: "12".to_string(),
            activation_height: 0,
            expiry_height: 100,
            revoked: false,
            policy_hash: e,
        }
    }
}

impl SessionPolicy {
    /// Check that a signer request matches this session's policy and the
    /// invariants the protocol requires of a signed payload. Returned errors
    /// surface a short reason key (e.g. `"route"`, `"family"`, `"fee"`) so
    /// the W3.2 CLI can map them to typed exit codes without re-parsing the
    /// message.
    pub fn authorize(&self, req: &SignerRequest) -> anyhow::Result<()> {
        self.validate()?;
        if !self.allowed_routes.iter().any(|r| r == &req.route) {
            anyhow::bail!("route {:?} not in allowed_routes", req.route);
        }
        if !self.allowed_family_ids.iter().any(|f| f == &req.family_id) {
            anyhow::bail!("family {:?} not in allowed_family_ids", req.family_id);
        }
        if !self
            .allowed_verifier_ids
            .iter()
            .any(|v| v == &req.verifier_id)
        {
            anyhow::bail!("verifier {:?} not in allowed_verifier_ids", req.verifier_id);
        }
        let fee = req
            .fee
            .parse::<u128>()
            .map_err(|e| anyhow::anyhow!("fee parse: {e}"))?;
        let max = self
            .max_fee_per_request
            .parse::<u128>()
            .map_err(|e| anyhow::anyhow!("max_fee_per_request parse: {e}"))?;
        if fee > max {
            anyhow::bail!("fee {} exceeds max_fee_per_request {}", fee, max);
        }
        validate_hex32("requestHash", &req.request_hash)?;
        if req.nonce.trim().is_empty() {
            anyhow::bail!("nonce must be non-empty");
        }
        Ok(())
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.can_withdraw {
            anyhow::bail!("session policy requires canWithdraw=false");
        }
        if self.can_transfer {
            anyhow::bail!("session policy requires canTransfer=false");
        }
        let _ = self.max_fee_per_request.parse::<u128>()?;
        let _ = self.daily_fee_cap.parse::<u128>()?;
        Ok(())
    }

    pub fn test_fixture() -> Self {
        Self {
            can_submit_work: true,
            can_pay_verification_fee: true,
            can_withdraw: false,
            can_transfer: false,
            allowed_routes: vec!["/verify-answer".to_string(), "/submit".to_string()],
            allowed_family_ids: vec!["boole.protocol-invariant.v01".to_string()],
            allowed_verifier_ids: vec!["lean-runner-v01".to_string()],
            max_fee_per_request: "12".to_string(),
            daily_fee_cap: "100".to_string(),
        }
    }
}

fn validate_hex32(field: &str, value: &str) -> anyhow::Result<()> {
    if Hex32::from_hex(value).is_err() {
        anyhow::bail!("{field} must be 32-byte lowercase hex");
    }
    Ok(())
}
