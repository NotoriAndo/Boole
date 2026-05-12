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
        Self {
            session_pk: a,
            owner_pk: b.clone(),
            agent_pk: c,
            fixed_reward_recipient: b,
            allowed_family_root: d.clone(),
            max_fee_per_request: "12".to_string(),
            activation_height: 0,
            expiry_height: 100,
            revoked: false,
            policy_hash: d,
        }
    }
}

impl SessionPolicy {
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
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        anyhow::bail!("{field} must be 32-byte lowercase hex");
    }
    Ok(())
}
