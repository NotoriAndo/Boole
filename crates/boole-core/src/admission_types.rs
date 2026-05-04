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
