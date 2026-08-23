//! BF.6 pure assignment/commit/reveal contracts.
//!
//! This module is deliberately data-only. It does not add a route, miner
//! loop, P2P frame, store, reward, block field, or activation switch.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::hash::{h_protocol, Hex32};
use crate::useful_work::push_field;

const NATIVE_TASK_ID_DOMAIN: &[u8] = b"boole.useful-work.native-task-id.v1";
const TASK_INSTANCE_ID_DOMAIN: &[u8] = b"boole.useful-work.task-instance-id.v1";
const ASSIGNMENT_DIGEST_DOMAIN: &[u8] = b"boole.useful-work.assignment-digest.v1";
const RESULT_COMMITMENT_DOMAIN: &[u8] = b"boole.useful-work.result-commitment.v1";
const COMMIT_DIGEST_DOMAIN: &[u8] = b"boole.useful-work.commit-digest.v1";
const RECEIPT_BRIDGE_DOMAIN: &[u8] = b"boole.useful-work.receipt-bridge.v1";

pub const BF6_ASSIGNMENT_SCHEMA: &str = "boole.useful-work.assignment.v1";
pub const BF6_COMMIT_SCHEMA: &str = "boole.useful-work.commit.v1";
pub const BF6_REVEAL_SCHEMA: &str = "boole.useful-work.reveal.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StableTaskId(Hex32);

impl StableTaskId {
    pub fn from_hex32(value: Hex32) -> Self {
        Self(value)
    }

    pub fn as_hex32(&self) -> Hex32 {
        self.0
    }

    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskInstanceId(Hex32);

impl TaskInstanceId {
    pub fn as_hex32(&self) -> Hex32 {
        self.0
    }

    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UsefulWorkBf6Error {
    #[error("malformed BF.6 json: {0}")]
    MalformedJson(String),
    #[error("unexpected schema in {field}")]
    UnexpectedSchema { field: &'static str },
    #[error("invalid digest in {field}")]
    InvalidDigest { field: &'static str },
    #[error("empty field {field}")]
    EmptyField { field: &'static str },
    #[error("declared stable task id does not match native task material")]
    StableTaskIdMismatch,
    #[error("declared task instance id does not match challenge/epoch/registry binding")]
    TaskInstanceIdMismatch,
    #[error("{field} is not bound to the assignment")]
    AssignmentBindingMismatch { field: &'static str },
    #[error("reveal does not open the committed result")]
    CommitmentMismatch,
    #[error("signed envelope is not bound to the assigned network")]
    NetworkBindingMismatch,
    #[error("signed envelope is not from the assigned signer")]
    SignerBindingMismatch,
    #[error("signed envelope signature is invalid")]
    InvalidSignature,
    #[error("BF.3 receipt belongs to an unassigned stable task")]
    ReceiptTaskNotAssigned,
    #[error("BF.3 receipt submission does not match the reveal")]
    ReceiptSubmissionMismatch,
}

impl UsefulWorkBf6Error {
    pub fn label(&self) -> &'static str {
        match self {
            UsefulWorkBf6Error::MalformedJson(_) => "malformed-json",
            UsefulWorkBf6Error::UnexpectedSchema { .. } => "unexpected-schema",
            UsefulWorkBf6Error::InvalidDigest { .. } => "invalid-digest",
            UsefulWorkBf6Error::EmptyField { .. } => "empty-field",
            UsefulWorkBf6Error::StableTaskIdMismatch => "task-id-mismatch",
            UsefulWorkBf6Error::TaskInstanceIdMismatch => "task-instance-id-mismatch",
            UsefulWorkBf6Error::AssignmentBindingMismatch { .. } => "assignment-binding-mismatch",
            UsefulWorkBf6Error::CommitmentMismatch => "commitment-mismatch",
            UsefulWorkBf6Error::NetworkBindingMismatch => "network-binding-mismatch",
            UsefulWorkBf6Error::SignerBindingMismatch => "signer-binding-mismatch",
            UsefulWorkBf6Error::InvalidSignature => "invalid-signature",
            UsefulWorkBf6Error::ReceiptTaskNotAssigned => "unassigned-task",
            UsefulWorkBf6Error::ReceiptSubmissionMismatch => "receipt-submission-mismatch",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTaskIdentity {
    family_version: String,
    template_id: Hex32,
    anchor_digest: Hex32,
}

impl NativeTaskIdentity {
    pub fn try_new(
        family_version: impl Into<String>,
        template_id: Hex32,
        anchor_digest: Hex32,
    ) -> Result<Self, UsefulWorkBf6Error> {
        let family_version = family_version.into();
        if family_version.is_empty() {
            return Err(UsefulWorkBf6Error::EmptyField {
                field: "familyVersion",
            });
        }
        Ok(Self {
            family_version,
            template_id,
            anchor_digest,
        })
    }

    /// The stable task identity deliberately excludes registry, challenge,
    /// epoch, network and reward data. Those belong to an assignment instance.
    pub fn task_id(&self) -> StableTaskId {
        let mut bytes = Vec::new();
        push_field(&mut bytes, self.family_version.as_bytes());
        push_field(&mut bytes, self.template_id.as_bytes());
        push_field(&mut bytes, self.anchor_digest.as_bytes());
        StableTaskId(h_protocol(NATIVE_TASK_ID_DOMAIN, &[&bytes]))
    }

    pub fn family_version(&self) -> &str {
        &self.family_version
    }

    pub fn template_id(&self) -> Hex32 {
        self.template_id
    }

    pub fn anchor_digest(&self) -> Hex32 {
        self.anchor_digest
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskInstanceBinding {
    task_id: StableTaskId,
    challenge_sha256: Hex32,
    epoch: u64,
    registry_digest: Hex32,
}

impl TaskInstanceBinding {
    pub fn new(
        task_id: StableTaskId,
        challenge_sha256: Hex32,
        epoch: u64,
        registry_digest: Hex32,
    ) -> Self {
        Self {
            task_id,
            challenge_sha256,
            epoch,
            registry_digest,
        }
    }

    pub fn task_instance_id(&self) -> TaskInstanceId {
        let mut bytes = Vec::new();
        push_field(&mut bytes, self.task_id.0.as_bytes());
        push_field(&mut bytes, self.challenge_sha256.as_bytes());
        push_field(&mut bytes, &self.epoch.to_be_bytes());
        push_field(&mut bytes, self.registry_digest.as_bytes());
        TaskInstanceId(h_protocol(TASK_INSTANCE_ID_DOMAIN, &[&bytes]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsefulWorkAssignment {
    native_task: NativeTaskIdentity,
    task_id: StableTaskId,
    task_instance_id: TaskInstanceId,
    challenge_sha256: Hex32,
    epoch: u64,
    registry_digest: Hex32,
    ticket_id: Hex32,
    network_id: String,
    assignee_pk: Hex32,
    reward_pk: Hex32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UsefulWorkAssignmentRaw {
    schema: String,
    family_version: String,
    template_id: String,
    anchor_digest: String,
    task_id: String,
    task_instance_id: String,
    challenge_sha256: String,
    epoch: u64,
    registry_digest: String,
    ticket_id: String,
    network_id: String,
    assignee_pk: String,
    reward_pk: String,
}

fn parse_digest(value: &str, field: &'static str) -> Result<Hex32, UsefulWorkBf6Error> {
    Hex32::from_hex(value).map_err(|_| UsefulWorkBf6Error::InvalidDigest { field })
}

fn require_text(value: String, field: &'static str) -> Result<String, UsefulWorkBf6Error> {
    if value.is_empty() {
        Err(UsefulWorkBf6Error::EmptyField { field })
    } else {
        Ok(value)
    }
}

impl UsefulWorkAssignment {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        native_task: NativeTaskIdentity,
        challenge_sha256: Hex32,
        epoch: u64,
        registry_digest: Hex32,
        ticket_id: Hex32,
        network_id: impl Into<String>,
        assignee_pk: Hex32,
        reward_pk: Hex32,
    ) -> Result<Self, UsefulWorkBf6Error> {
        let network_id = require_text(network_id.into(), "networkId")?;
        let task_id = native_task.task_id();
        let task_instance_id =
            TaskInstanceBinding::new(task_id, challenge_sha256, epoch, registry_digest)
                .task_instance_id();
        Ok(Self {
            native_task,
            task_id,
            task_instance_id,
            challenge_sha256,
            epoch,
            registry_digest,
            ticket_id,
            network_id,
            assignee_pk,
            reward_pk,
        })
    }

    pub fn from_json_bytes(raw: &[u8]) -> Result<Self, UsefulWorkBf6Error> {
        let value: UsefulWorkAssignmentRaw = serde_json::from_slice(raw)
            .map_err(|error| UsefulWorkBf6Error::MalformedJson(error.to_string()))?;
        Self::from_raw(value)
    }

    fn from_raw(value: UsefulWorkAssignmentRaw) -> Result<Self, UsefulWorkBf6Error> {
        if value.schema != BF6_ASSIGNMENT_SCHEMA {
            return Err(UsefulWorkBf6Error::UnexpectedSchema {
                field: "assignment.schema",
            });
        }
        let native_task = NativeTaskIdentity::try_new(
            value.family_version,
            parse_digest(&value.template_id, "templateId")?,
            parse_digest(&value.anchor_digest, "anchorDigest")?,
        )?;
        let parsed_task_id = StableTaskId(parse_digest(&value.task_id, "taskId")?);
        if parsed_task_id != native_task.task_id() {
            return Err(UsefulWorkBf6Error::StableTaskIdMismatch);
        }
        let assignment = Self::try_new(
            native_task,
            parse_digest(&value.challenge_sha256, "challengeSha256")?,
            value.epoch,
            parse_digest(&value.registry_digest, "registryDigest")?,
            parse_digest(&value.ticket_id, "ticketId")?,
            value.network_id,
            parse_digest(&value.assignee_pk, "assigneePk")?,
            parse_digest(&value.reward_pk, "rewardPk")?,
        )?;
        let parsed_instance =
            TaskInstanceId(parse_digest(&value.task_instance_id, "taskInstanceId")?);
        if parsed_instance != assignment.task_instance_id {
            return Err(UsefulWorkBf6Error::TaskInstanceIdMismatch);
        }
        Ok(assignment)
    }

    fn to_raw(&self) -> UsefulWorkAssignmentRaw {
        UsefulWorkAssignmentRaw {
            schema: BF6_ASSIGNMENT_SCHEMA.to_string(),
            family_version: self.native_task.family_version.clone(),
            template_id: self.native_task.template_id.to_hex(),
            anchor_digest: self.native_task.anchor_digest.to_hex(),
            task_id: self.task_id.to_hex(),
            task_instance_id: self.task_instance_id.to_hex(),
            challenge_sha256: self.challenge_sha256.to_hex(),
            epoch: self.epoch,
            registry_digest: self.registry_digest.to_hex(),
            ticket_id: self.ticket_id.to_hex(),
            network_id: self.network_id.clone(),
            assignee_pk: self.assignee_pk.to_hex(),
            reward_pk: self.reward_pk.to_hex(),
        }
    }

    pub fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self.to_raw()).expect("assignment serialization is infallible")
    }

    pub fn to_canonical_json_bytes(&self) -> Vec<u8> {
        crate::canonical_json::canonicalize(&self.to_json_value())
    }

    pub fn assignment_digest(&self) -> Hex32 {
        h_protocol(ASSIGNMENT_DIGEST_DOMAIN, &[&self.to_canonical_json_bytes()])
    }

    pub fn native_task(&self) -> &NativeTaskIdentity {
        &self.native_task
    }

    pub fn task_id(&self) -> StableTaskId {
        self.task_id
    }

    pub fn task_instance_id(&self) -> TaskInstanceId {
        self.task_instance_id
    }

    pub fn challenge_sha256(&self) -> Hex32 {
        self.challenge_sha256
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn registry_digest(&self) -> Hex32 {
        self.registry_digest
    }

    pub fn network_id(&self) -> &str {
        &self.network_id
    }

    pub fn assignee_pk(&self) -> Hex32 {
        self.assignee_pk
    }

    pub fn reward_pk(&self) -> Hex32 {
        self.reward_pk
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsefulWorkCommit {
    assignment_digest: Hex32,
    task_id: StableTaskId,
    task_instance_id: TaskInstanceId,
    network_id: String,
    assignee_pk: Hex32,
    reward_pk: Hex32,
    result_commitment: Hex32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UsefulWorkCommitRaw {
    schema: String,
    assignment_digest: String,
    task_id: String,
    task_instance_id: String,
    network_id: String,
    assignee_pk: String,
    reward_pk: String,
    result_commitment: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsefulWorkReveal {
    assignment_digest: Hex32,
    commit_digest: Hex32,
    task_id: StableTaskId,
    task_instance_id: TaskInstanceId,
    network_id: String,
    assignee_pk: Hex32,
    reward_pk: Hex32,
    submission_id: Hex32,
    candidate_digest: Hex32,
    nonce: Hex32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UsefulWorkRevealRaw {
    schema: String,
    assignment_digest: String,
    commit_digest: String,
    task_id: String,
    task_instance_id: String,
    network_id: String,
    assignee_pk: String,
    reward_pk: String,
    submission_id: String,
    candidate_digest: String,
    nonce: String,
}

#[allow(clippy::too_many_arguments)]
pub fn bf6_result_commitment(
    assignment_digest: &Hex32,
    task_id: StableTaskId,
    task_instance_id: TaskInstanceId,
    network_id: &str,
    assignee_pk: &Hex32,
    reward_pk: &Hex32,
    submission_id: &Hex32,
    candidate_digest: &Hex32,
    nonce: &Hex32,
) -> Hex32 {
    let mut bytes = Vec::new();
    push_field(&mut bytes, assignment_digest.as_bytes());
    push_field(&mut bytes, task_id.0.as_bytes());
    push_field(&mut bytes, task_instance_id.0.as_bytes());
    push_field(&mut bytes, network_id.as_bytes());
    push_field(&mut bytes, assignee_pk.as_bytes());
    push_field(&mut bytes, reward_pk.as_bytes());
    push_field(&mut bytes, submission_id.as_bytes());
    push_field(&mut bytes, candidate_digest.as_bytes());
    push_field(&mut bytes, nonce.as_bytes());
    h_protocol(RESULT_COMMITMENT_DOMAIN, &[&bytes])
}

impl UsefulWorkCommit {
    pub fn try_new(
        assignment: &UsefulWorkAssignment,
        submission_id: Hex32,
        candidate_digest: Hex32,
        nonce: Hex32,
    ) -> Result<Self, UsefulWorkBf6Error> {
        let assignment_digest = assignment.assignment_digest();
        let result_commitment = bf6_result_commitment(
            &assignment_digest,
            assignment.task_id,
            assignment.task_instance_id,
            &assignment.network_id,
            &assignment.assignee_pk,
            &assignment.reward_pk,
            &submission_id,
            &candidate_digest,
            &nonce,
        );
        Ok(Self {
            assignment_digest,
            task_id: assignment.task_id,
            task_instance_id: assignment.task_instance_id,
            network_id: assignment.network_id.clone(),
            assignee_pk: assignment.assignee_pk,
            reward_pk: assignment.reward_pk,
            result_commitment,
        })
    }

    pub fn from_json_bytes(raw: &[u8]) -> Result<Self, UsefulWorkBf6Error> {
        let value: UsefulWorkCommitRaw = serde_json::from_slice(raw)
            .map_err(|error| UsefulWorkBf6Error::MalformedJson(error.to_string()))?;
        Self::from_raw(value)
    }

    fn from_raw(value: UsefulWorkCommitRaw) -> Result<Self, UsefulWorkBf6Error> {
        if value.schema != BF6_COMMIT_SCHEMA {
            return Err(UsefulWorkBf6Error::UnexpectedSchema {
                field: "commit.schema",
            });
        }
        Ok(Self {
            assignment_digest: parse_digest(&value.assignment_digest, "assignmentDigest")?,
            task_id: StableTaskId(parse_digest(&value.task_id, "taskId")?),
            task_instance_id: TaskInstanceId(parse_digest(
                &value.task_instance_id,
                "taskInstanceId",
            )?),
            network_id: require_text(value.network_id, "networkId")?,
            assignee_pk: parse_digest(&value.assignee_pk, "assigneePk")?,
            reward_pk: parse_digest(&value.reward_pk, "rewardPk")?,
            result_commitment: parse_digest(&value.result_commitment, "resultCommitment")?,
        })
    }

    fn to_raw(&self) -> UsefulWorkCommitRaw {
        UsefulWorkCommitRaw {
            schema: BF6_COMMIT_SCHEMA.to_string(),
            assignment_digest: self.assignment_digest.to_hex(),
            task_id: self.task_id.to_hex(),
            task_instance_id: self.task_instance_id.to_hex(),
            network_id: self.network_id.clone(),
            assignee_pk: self.assignee_pk.to_hex(),
            reward_pk: self.reward_pk.to_hex(),
            result_commitment: self.result_commitment.to_hex(),
        }
    }

    pub fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self.to_raw()).expect("commit serialization is infallible")
    }

    pub fn to_canonical_json_bytes(&self) -> Vec<u8> {
        crate::canonical_json::canonicalize(&self.to_json_value())
    }

    pub fn commit_digest(&self) -> Hex32 {
        h_protocol(COMMIT_DIGEST_DOMAIN, &[&self.to_canonical_json_bytes()])
    }

    pub fn result_commitment(&self) -> Hex32 {
        self.result_commitment
    }
}

impl UsefulWorkReveal {
    pub fn try_new(
        assignment: &UsefulWorkAssignment,
        commit: &UsefulWorkCommit,
        submission_id: Hex32,
        candidate_digest: Hex32,
        nonce: Hex32,
    ) -> Result<Self, UsefulWorkBf6Error> {
        validate_commit(assignment, commit)?;
        let reveal = Self {
            assignment_digest: assignment.assignment_digest(),
            commit_digest: commit.commit_digest(),
            task_id: assignment.task_id,
            task_instance_id: assignment.task_instance_id,
            network_id: assignment.network_id.clone(),
            assignee_pk: assignment.assignee_pk,
            reward_pk: assignment.reward_pk,
            submission_id,
            candidate_digest,
            nonce,
        };
        validate_reveal(assignment, commit, &reveal)?;
        Ok(reveal)
    }

    pub fn from_json_bytes(raw: &[u8]) -> Result<Self, UsefulWorkBf6Error> {
        let value: UsefulWorkRevealRaw = serde_json::from_slice(raw)
            .map_err(|error| UsefulWorkBf6Error::MalformedJson(error.to_string()))?;
        Self::from_raw(value)
    }

    fn from_raw(value: UsefulWorkRevealRaw) -> Result<Self, UsefulWorkBf6Error> {
        if value.schema != BF6_REVEAL_SCHEMA {
            return Err(UsefulWorkBf6Error::UnexpectedSchema {
                field: "reveal.schema",
            });
        }
        Ok(Self {
            assignment_digest: parse_digest(&value.assignment_digest, "assignmentDigest")?,
            commit_digest: parse_digest(&value.commit_digest, "commitDigest")?,
            task_id: StableTaskId(parse_digest(&value.task_id, "taskId")?),
            task_instance_id: TaskInstanceId(parse_digest(
                &value.task_instance_id,
                "taskInstanceId",
            )?),
            network_id: require_text(value.network_id, "networkId")?,
            assignee_pk: parse_digest(&value.assignee_pk, "assigneePk")?,
            reward_pk: parse_digest(&value.reward_pk, "rewardPk")?,
            submission_id: parse_digest(&value.submission_id, "submissionId")?,
            candidate_digest: parse_digest(&value.candidate_digest, "candidateDigest")?,
            nonce: parse_digest(&value.nonce, "nonce")?,
        })
    }

    fn to_raw(&self) -> UsefulWorkRevealRaw {
        UsefulWorkRevealRaw {
            schema: BF6_REVEAL_SCHEMA.to_string(),
            assignment_digest: self.assignment_digest.to_hex(),
            commit_digest: self.commit_digest.to_hex(),
            task_id: self.task_id.to_hex(),
            task_instance_id: self.task_instance_id.to_hex(),
            network_id: self.network_id.clone(),
            assignee_pk: self.assignee_pk.to_hex(),
            reward_pk: self.reward_pk.to_hex(),
            submission_id: self.submission_id.to_hex(),
            candidate_digest: self.candidate_digest.to_hex(),
            nonce: self.nonce.to_hex(),
        }
    }

    pub fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self.to_raw()).expect("reveal serialization is infallible")
    }

    pub fn to_canonical_json_bytes(&self) -> Vec<u8> {
        crate::canonical_json::canonicalize(&self.to_json_value())
    }

    pub fn submission_id(&self) -> Hex32 {
        self.submission_id
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedEnvelopeRaw<T> {
    schema: String,
    payload: T,
    pk: String,
    signature: String,
    network_id: String,
}

fn validate_signed_payload<T: Serialize>(
    envelope: &SignedEnvelopeRaw<T>,
    expected_network_id: &str,
    expected_signer_pk: Hex32,
) -> Result<(), UsefulWorkBf6Error> {
    if envelope.schema != crate::SIGNED_ENVELOPE_SCHEMA {
        return Err(UsefulWorkBf6Error::UnexpectedSchema {
            field: "envelope.schema",
        });
    }
    if envelope.network_id != expected_network_id {
        return Err(UsefulWorkBf6Error::NetworkBindingMismatch);
    }
    let signer = parse_digest(&envelope.pk, "envelope.pk")?;
    if signer != expected_signer_pk {
        return Err(UsefulWorkBf6Error::SignerBindingMismatch);
    }
    let payload = serde_json::to_value(&envelope.payload)
        .map_err(|error| UsefulWorkBf6Error::MalformedJson(error.to_string()))?;
    let valid = crate::verify_signature_with_network(
        &envelope.pk,
        &envelope.signature,
        &payload,
        Some(&envelope.network_id),
    )
    .map_err(|_| UsefulWorkBf6Error::InvalidSignature)?;
    if !valid {
        return Err(UsefulWorkBf6Error::InvalidSignature);
    }
    Ok(())
}

pub fn decode_signed_assignment(
    raw: &[u8],
    expected_network_id: &str,
    expected_authority_pk: Hex32,
) -> Result<UsefulWorkAssignment, UsefulWorkBf6Error> {
    let envelope: SignedEnvelopeRaw<UsefulWorkAssignmentRaw> = serde_json::from_slice(raw)
        .map_err(|error| UsefulWorkBf6Error::MalformedJson(error.to_string()))?;
    validate_signed_payload(&envelope, expected_network_id, expected_authority_pk)?;
    let assignment = UsefulWorkAssignment::from_raw(envelope.payload)?;
    if assignment.network_id != expected_network_id {
        return Err(UsefulWorkBf6Error::NetworkBindingMismatch);
    }
    Ok(assignment)
}

pub fn decode_signed_commit(
    raw: &[u8],
    assignment: &UsefulWorkAssignment,
) -> Result<UsefulWorkCommit, UsefulWorkBf6Error> {
    let envelope: SignedEnvelopeRaw<UsefulWorkCommitRaw> = serde_json::from_slice(raw)
        .map_err(|error| UsefulWorkBf6Error::MalformedJson(error.to_string()))?;
    validate_signed_payload(&envelope, &assignment.network_id, assignment.assignee_pk)?;
    let commit = UsefulWorkCommit::from_raw(envelope.payload)?;
    validate_commit(assignment, &commit)?;
    Ok(commit)
}

pub fn decode_signed_reveal(
    raw: &[u8],
    assignment: &UsefulWorkAssignment,
    commit: &UsefulWorkCommit,
) -> Result<UsefulWorkReveal, UsefulWorkBf6Error> {
    let envelope: SignedEnvelopeRaw<UsefulWorkRevealRaw> = serde_json::from_slice(raw)
        .map_err(|error| UsefulWorkBf6Error::MalformedJson(error.to_string()))?;
    validate_signed_payload(&envelope, &assignment.network_id, assignment.assignee_pk)?;
    let reveal = UsefulWorkReveal::from_raw(envelope.payload)?;
    validate_reveal(assignment, commit, &reveal)?;
    Ok(reveal)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssignedReceiptBridge {
    task_id: StableTaskId,
    task_instance_id: TaskInstanceId,
    assignment_digest: Hex32,
    commit_digest: Hex32,
    receipt_digest: Hex32,
}

impl AssignedReceiptBridge {
    pub fn task_id(&self) -> StableTaskId {
        self.task_id
    }

    pub fn task_instance_id(&self) -> TaskInstanceId {
        self.task_instance_id
    }

    pub fn receipt_digest(&self) -> Hex32 {
        self.receipt_digest
    }

    pub fn bridge_digest(&self) -> Hex32 {
        let mut bytes = Vec::new();
        push_field(&mut bytes, self.task_id.0.as_bytes());
        push_field(&mut bytes, self.task_instance_id.0.as_bytes());
        push_field(&mut bytes, self.assignment_digest.as_bytes());
        push_field(&mut bytes, self.commit_digest.as_bytes());
        push_field(&mut bytes, self.receipt_digest.as_bytes());
        h_protocol(RECEIPT_BRIDGE_DOMAIN, &[&bytes])
    }
}

/// Bridge an already node-issued BF.3 receipt into one exact BF.6 assignment.
/// The receipt contributes the stable task and submission identities; the
/// assignment contributes the challenge-bound instance. Neither id may stand
/// in for the other.
pub fn validate_native_receipt_bridge(
    assignment: &UsefulWorkAssignment,
    commit: &UsefulWorkCommit,
    reveal: &UsefulWorkReveal,
    receipt: &crate::useful_product::VerificationReceipt,
) -> Result<AssignedReceiptBridge, UsefulWorkBf6Error> {
    validate_reveal(assignment, commit, reveal)?;
    if receipt.task_id != assignment.task_id.0 {
        return Err(UsefulWorkBf6Error::ReceiptTaskNotAssigned);
    }
    if receipt.submission_id != reveal.submission_id {
        return Err(UsefulWorkBf6Error::ReceiptSubmissionMismatch);
    }
    Ok(AssignedReceiptBridge {
        task_id: assignment.task_id,
        task_instance_id: assignment.task_instance_id,
        assignment_digest: assignment.assignment_digest(),
        commit_digest: commit.commit_digest(),
        receipt_digest: receipt.receipt_digest(),
    })
}

fn require_same<T: PartialEq>(
    actual: &T,
    expected: &T,
    field: &'static str,
) -> Result<(), UsefulWorkBf6Error> {
    if actual != expected {
        Err(UsefulWorkBf6Error::AssignmentBindingMismatch { field })
    } else {
        Ok(())
    }
}

pub fn validate_commit(
    assignment: &UsefulWorkAssignment,
    commit: &UsefulWorkCommit,
) -> Result<(), UsefulWorkBf6Error> {
    require_same(
        &commit.assignment_digest,
        &assignment.assignment_digest(),
        "assignmentDigest",
    )?;
    require_same(&commit.task_id, &assignment.task_id, "taskId")?;
    require_same(
        &commit.task_instance_id,
        &assignment.task_instance_id,
        "taskInstanceId",
    )?;
    require_same(&commit.network_id, &assignment.network_id, "networkId")?;
    require_same(&commit.assignee_pk, &assignment.assignee_pk, "assigneePk")?;
    require_same(&commit.reward_pk, &assignment.reward_pk, "rewardPk")?;
    Ok(())
}

pub fn validate_reveal(
    assignment: &UsefulWorkAssignment,
    commit: &UsefulWorkCommit,
    reveal: &UsefulWorkReveal,
) -> Result<(), UsefulWorkBf6Error> {
    validate_commit(assignment, commit)?;
    require_same(
        &reveal.assignment_digest,
        &assignment.assignment_digest(),
        "assignmentDigest",
    )?;
    require_same(
        &reveal.commit_digest,
        &commit.commit_digest(),
        "commitDigest",
    )?;
    require_same(&reveal.task_id, &assignment.task_id, "taskId")?;
    require_same(
        &reveal.task_instance_id,
        &assignment.task_instance_id,
        "taskInstanceId",
    )?;
    require_same(&reveal.network_id, &assignment.network_id, "networkId")?;
    require_same(&reveal.assignee_pk, &assignment.assignee_pk, "assigneePk")?;
    require_same(&reveal.reward_pk, &assignment.reward_pk, "rewardPk")?;

    let expected = bf6_result_commitment(
        &reveal.assignment_digest,
        reveal.task_id,
        reveal.task_instance_id,
        &reveal.network_id,
        &reveal.assignee_pk,
        &reveal.reward_pk,
        &reveal.submission_id,
        &reveal.candidate_digest,
        &reveal.nonce,
    );
    if expected != commit.result_commitment {
        return Err(UsefulWorkBf6Error::CommitmentMismatch);
    }
    Ok(())
}
