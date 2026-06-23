use crate::{Hex32, RateLimitRejectReason, SharePoolRejectReason, ValidationReason};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSubmission {
    pub c_hex: String,
    pub pk_hex: String,
    pub n_hex: String,
    pub j_hex: String,
    pub nonce_s_hex: String,
    pub c: Hex32,
    pub pk: Hex32,
    pub n: Hex32,
    pub j: Hex32,
    pub nonce_s: Hex32,
    pub package_bytes: Vec<u8>,
    /// N0.4b (Path 2) — OPTIONAL family seed hex. Lets the node persist the
    /// seed on the block so `deep_verify_block` can re-derive the share's
    /// canonical Lean source and recompute the canon. Empty when the
    /// submitter omits it (pre-N0.4b miners, submit-lean/bounty flows); it
    /// does not participate in admission.
    pub seed_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionDecision {
    Accepted {
        share_hash: Hex32,
    },
    Rejected {
        status: AdmissionStatus,
        error: AdmissionError,
        rejection: RejectionReason,
    },
}

impl AdmissionDecision {
    /// Stable machine-readable rejection code, or `None` when accepted. The
    /// node surfaces this as an additive `code` field on `/submit` rejects so
    /// clients can branch without parsing Debug prose (N0-pre.10).
    pub fn reject_code(&self) -> Option<&'static str> {
        match self {
            Self::Accepted { .. } => None,
            Self::Rejected { rejection, .. } => Some(rejection.code()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionStatus {
    BadRequest,
    UnprocessableEntity,
    RateLimited,
}

impl AdmissionStatus {
    pub(crate) fn code(self) -> u16 {
        match self {
            Self::BadRequest => 400,
            Self::UnprocessableEntity => 422,
            Self::RateLimited => 429,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionError {
    MissingField { field: String },
    InvalidFieldType { field: String, expected: String },
    BadHex { field: String, detail: String },
    Ticket { reason: TicketRejectReason },
    Validator { reason: ValidationReason },
    SubmitPow { reason: SubmitPowRejectReason },
    RateLimited { reason: RateLimitRejectReason },
    SharePool { reason: SharePoolRejectReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketAdmissionResult {
    Allowed,
    Rejected { reason: TicketRejectReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketRejectReason {
    AboveTTicket,
    Unobserved,
}

impl TicketRejectReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AboveTTicket => "above_T_ticket",
            Self::Unobserved => "unobserved",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitPowRejectReason {
    AboveTSubmit,
}

impl SubmitPowRejectReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AboveTSubmit => "above_T_submit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectionReason {
    BadRequest { field: String },
    Decode { field: String, detail: String },
    Ticket { detail: TicketRejectReason },
    Validator { reason: ValidationReason },
    SubmitPow { detail: SubmitPowRejectReason },
    RateLimit { quota: RateLimitRejectReason },
    SharePool { detail: SharePoolRejectReason },
}

impl RejectionReason {
    /// Stable, machine-readable code for this rejection. Unlike the Debug
    /// rendering, this string is part of the wire contract: a client may
    /// branch on it (e.g. the miner treats `"stale_c"` as a mid-cycle
    /// head-advance signal) without depending on human-readable prose. The
    /// SharePool family delegates to `SharePoolRejectReason::as_str` so the
    /// `stale_c` code is single-sourced.
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest { .. } => "bad_request",
            Self::Decode { .. } => "decode",
            Self::Ticket { detail } => detail.as_str(),
            Self::Validator { .. } => "validator_rejected",
            Self::SubmitPow { detail } => detail.as_str(),
            Self::RateLimit { .. } => "rate_limited",
            Self::SharePool { detail } => detail.as_str(),
        }
    }
}
