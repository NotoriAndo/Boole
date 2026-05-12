use crate::llm_driver::RejectionReason;

pub const PROOF_BODY_CONTRACT_VERSION: &str = "boole-proof-body-v1";
pub const PROOF_CANONICALIZER_VERSION: &str = "boole-proof-canonicalizer-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofEnvelope {
    /// The only field that may become a proof candidate. Runtime stdout/stderr
    /// are intentionally kept separate so logs, warnings, and telemetry cannot
    /// be silently mined as Lean source.
    pub answer: String,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofTransport {
    /// Legacy/shared answer envelope for drivers that already return the model's
    /// declared answer text. This is not a model-specific escape hatch: all
    /// providers still pass through ProofIntakeV1.
    PlainText(String),
    Envelope(ProofEnvelope),
}

impl ProofTransport {
    pub fn into_envelope(self) -> Result<ProofEnvelope, RejectionReason> {
        match self {
            ProofTransport::PlainText(answer) => Ok(ProofEnvelope {
                answer,
                stdout: None,
                stderr: None,
            }),
            ProofTransport::Envelope(envelope) => Ok(envelope),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofCandidate {
    pub proof_source: String,
    pub contract_version: &'static str,
    pub canonicalizer_version: &'static str,
}

pub struct ProofIntakeV1;

impl ProofIntakeV1 {
    pub fn extract(envelope: ProofEnvelope) -> Result<ProofCandidate, RejectionReason> {
        let proof_source = canonicalize_answer_channel(&envelope.answer)?;
        Ok(ProofCandidate {
            proof_source,
            contract_version: PROOF_BODY_CONTRACT_VERSION,
            canonicalizer_version: PROOF_CANONICALIZER_VERSION,
        })
    }
}

pub fn extract_proof_source(raw_answer: &str) -> Result<String, RejectionReason> {
    ProofTransport::PlainText(raw_answer.to_string())
        .into_envelope()
        .and_then(ProofIntakeV1::extract)
        .map(|candidate| candidate.proof_source)
}

fn canonicalize_answer_channel(raw: &str) -> Result<String, RejectionReason> {
    if raw.trim().is_empty() {
        return Err(RejectionReason::EmptyResponse);
    }
    let body: &str = match find_lean_fenced_block(raw) {
        Some(inner) => inner,
        None => raw,
    };
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(RejectionReason::NoProofBlock);
    }
    if violates_proof_body_contract(trimmed) {
        return Err(RejectionReason::ContractFailed);
    }
    Ok(trimmed.to_string())
}

fn violates_proof_body_contract(source: &str) -> bool {
    let first_non_empty = source.lines().find(|line| !line.trim().is_empty());
    if first_non_empty
        .map(|line| {
            let line = line.trim_start();
            line.starts_with("theorem ")
                || line.starts_with("lemma ")
                || line.starts_with("def ")
                || line.starts_with("example ")
                || line.starts_with("import ")
                || line.starts_with("namespace ")
                || line.starts_with("end ")
                || line.starts_with("#")
                || line.starts_with("* ")
                || line.starts_with("- ")
                || line.starts_with("WARNING")
                || line.starts_with("Warning")
                || line.starts_with("warning:")
                || line.starts_with("error:")
                || line.starts_with("stderr:")
                || line.starts_with("stdout:")
                || starts_with_bare_tactic_or_calc(line)
        })
        .unwrap_or(false)
    {
        return true;
    }

    source.lines().any(|line| {
        line.split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .any(|tok| tok == "sorry" || tok == "admit")
    })
}

fn starts_with_bare_tactic_or_calc(line: &str) -> bool {
    let first = line
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .find(|tok| !tok.is_empty());
    matches!(
        first,
        Some("apply" | "rw" | "intro" | "have" | "exact" | "calc")
    )
}

fn find_lean_fenced_block(raw: &str) -> Option<&str> {
    let start = raw.find("```")?;
    let after_open = &raw[start + 3..];
    let after_lang = if let Some(rest) = after_open.strip_prefix("lean4") {
        rest
    } else if let Some(rest) = after_open.strip_prefix("lean") {
        rest
    } else {
        after_open
    };
    let body_start = after_lang
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(i, _)| i)
        .unwrap_or(after_lang.len());
    let after_ws = &after_lang[body_start..];
    let close_rel = after_ws.find("```")?;
    Some(&after_ws[..close_rel])
}
