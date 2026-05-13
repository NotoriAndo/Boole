use serde::{Deserialize, Serialize};

use crate::ReceiptCommitment;

pub const AGENT_PASSPORT_EVENT_SCHEMA: &str = "boole.agent.event.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPassportEvent {
    pub schema: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_pk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward_recipient: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl AgentPassportEvent {
    pub fn work_accepted(receipt: &ReceiptCommitment) -> Self {
        Self {
            schema: AGENT_PASSPORT_EVENT_SCHEMA.to_string(),
            kind: "workAccepted".to_string(),
            agent_pk: Some(receipt.agent_pk.clone()),
            family_id: Some(receipt.family_id.clone()),
            receipt_id: Some(receipt.receipt_id.clone()),
            reward_recipient: None,
            amount: None,
            reason: None,
        }
    }

    pub fn work_rejected(receipt: &ReceiptCommitment) -> Self {
        Self {
            schema: AGENT_PASSPORT_EVENT_SCHEMA.to_string(),
            kind: "workRejected".to_string(),
            agent_pk: Some(receipt.agent_pk.clone()),
            family_id: Some(receipt.family_id.clone()),
            receipt_id: Some(receipt.receipt_id.clone()),
            reward_recipient: None,
            amount: None,
            reason: None,
        }
    }

    pub fn reward_credited(
        reward_recipient: impl Into<String>,
        amount: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            schema: AGENT_PASSPORT_EVENT_SCHEMA.to_string(),
            kind: "rewardCredited".to_string(),
            agent_pk: None,
            family_id: None,
            receipt_id: None,
            reward_recipient: Some(reward_recipient.into()),
            amount: Some(amount.into()),
            reason: Some(reason.into()),
        }
    }
}

pub fn agent_passport_events_for_receipt(receipt: &ReceiptCommitment) -> Vec<AgentPassportEvent> {
    match receipt.result.as_str() {
        "accepted" => vec![
            AgentPassportEvent::work_accepted(receipt),
            AgentPassportEvent::reward_credited(
                receipt.reward_recipient.clone(),
                receipt.fee_charged.clone(),
                "verify_answer_mock_fee",
            ),
        ],
        "rejected" => vec![AgentPassportEvent::work_rejected(receipt)],
        _ => Vec::new(),
    }
}
