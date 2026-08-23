//! Strict raw-answer boundary for the closed-local native-shadow route.

use serde::Deserialize;
use sha2::{Digest, Sha256};

const SUBMISSION_SCHEMA: &str = "boole.native-shadow.submission.v1";
const MAX_RAW_ANSWER_BYTES: usize = 16_384;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NativeShadowSubmission {
    schema: String,
    family_version: String,
    template_id: String,
    challenge_sha256: String,
    epoch: u64,
    raw_answer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum NativeShadowSubmissionError {
    #[error("submission JSON is malformed: {0}")]
    Malformed(String),
    #[error("submission schema is not boole.native-shadow.submission.v1")]
    WrongSchema,
    #[error("submission familyVersion is invalid")]
    InvalidFamilyVersion,
    #[error("submission {0} must be 64 lowercase hexadecimal characters")]
    InvalidDigest(&'static str),
    #[error("rawAnswer exceeds the 16384-byte ceiling")]
    RawAnswerTooLarge,
}

impl NativeShadowSubmission {
    pub(crate) fn parse_strict(bytes: &[u8]) -> Result<Self, NativeShadowSubmissionError> {
        let submission: Self = serde_json::from_slice(bytes)
            .map_err(|error| NativeShadowSubmissionError::Malformed(error.to_string()))?;
        if submission.schema != SUBMISSION_SCHEMA {
            return Err(NativeShadowSubmissionError::WrongSchema);
        }
        if submission.family_version.is_empty() || submission.family_version.len() > 256 {
            return Err(NativeShadowSubmissionError::InvalidFamilyVersion);
        }
        for (name, digest) in [
            ("templateId", submission.template_id.as_str()),
            ("challengeSha256", submission.challenge_sha256.as_str()),
        ] {
            if !is_lower_sha256_hex(digest) {
                return Err(NativeShadowSubmissionError::InvalidDigest(name));
            }
        }
        if submission.raw_answer.len() > MAX_RAW_ANSWER_BYTES {
            return Err(NativeShadowSubmissionError::RawAnswerTooLarge);
        }
        Ok(submission)
    }

    pub(crate) fn family_version(&self) -> &str {
        &self.family_version
    }

    pub(crate) fn template_id(&self) -> &str {
        &self.template_id
    }

    pub(crate) fn challenge_sha256(&self) -> &str {
        &self.challenge_sha256
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }

    pub(crate) fn raw_answer_bytes(&self) -> &[u8] {
        self.raw_answer.as_bytes()
    }

    pub(crate) fn candidate_digest_hex(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(self.raw_answer_bytes());
        hex::encode(digest.finalize())
    }

    pub(crate) fn submission_digest_hex(&self) -> String {
        boole_native_shadow_protocol::submission_digest_hex(
            self.family_version(),
            self.template_id(),
            self.challenge_sha256(),
            self.epoch(),
            self.raw_answer_bytes(),
        )
        .expect("strict submission validation already checked the digest contract")
    }
}

fn is_lower_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: char) -> String {
        byte.to_string().repeat(64)
    }

    fn valid_json(raw_answer: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema": SUBMISSION_SCHEMA,
            "familyVersion": "TUPLE-STRUCT-PROJECT/RUST-TUPLE-STRUCT-PROJECT-V1",
            "templateId": digest('a'),
            "challengeSha256": digest('b'),
            "epoch": 7,
            "rawAnswer": raw_answer,
        }))
        .expect("submission JSON")
    }

    #[test]
    fn strict_submission_accepts_exact_six_field_contract() {
        let submission = NativeShadowSubmission::parse_strict(&valid_json("fn answer() {}"))
            .expect("exact contract");
        assert_eq!(submission.schema, SUBMISSION_SCHEMA);
        assert_eq!(submission.epoch, 7);
        assert_eq!(submission.raw_answer, "fn answer() {}");
    }

    #[test]
    fn verdict_receipt_and_other_authority_injection_are_rejected() {
        for forbidden in [
            "verdict",
            "receipt",
            "checkerDigest",
            "policyDigest",
            "anchorDigest",
            "expectedAnswer",
            "witness",
        ] {
            let mut value: serde_json::Value =
                serde_json::from_slice(&valid_json("fn answer() {}")).expect("base submission");
            value[forbidden] = serde_json::json!("attacker-controlled");
            let encoded = serde_json::to_vec(&value).expect("encoded attack");
            assert!(
                matches!(
                    NativeShadowSubmission::parse_strict(&encoded),
                    Err(NativeShadowSubmissionError::Malformed(_))
                ),
                "accepted forbidden field {forbidden}"
            );
        }
    }

    #[test]
    fn duplicate_missing_float_and_trailing_json_are_rejected() {
        let duplicate = format!(
            r#"{{"schema":"{SUBMISSION_SCHEMA}","familyVersion":"F","templateId":"{}","challengeSha256":"{}","epoch":1,"epoch":2,"rawAnswer":"x"}}"#,
            digest('a'),
            digest('b')
        );
        let missing = format!(
            r#"{{"schema":"{SUBMISSION_SCHEMA}","familyVersion":"F","templateId":"{}","challengeSha256":"{}","epoch":1}}"#,
            digest('a'),
            digest('b')
        );
        let float_epoch = format!(
            r#"{{"schema":"{SUBMISSION_SCHEMA}","familyVersion":"F","templateId":"{}","challengeSha256":"{}","epoch":1.0,"rawAnswer":"x"}}"#,
            digest('a'),
            digest('b')
        );
        let trailing = [valid_json("x"), b"{}".to_vec()].concat();

        for malformed in [
            duplicate.into_bytes(),
            missing.into_bytes(),
            float_epoch.into_bytes(),
            trailing,
        ] {
            assert!(matches!(
                NativeShadowSubmission::parse_strict(&malformed),
                Err(NativeShadowSubmissionError::Malformed(_))
            ));
        }
    }

    #[test]
    fn identity_and_raw_answer_limits_are_fail_closed() {
        for (field, value) in [
            (
                "schema",
                serde_json::json!("boole.native-shadow.submission.v0"),
            ),
            ("familyVersion", serde_json::json!("")),
            ("familyVersion", serde_json::json!("f".repeat(257))),
            ("templateId", serde_json::json!("A".repeat(64))),
            ("templateId", serde_json::json!("a".repeat(63))),
            ("challengeSha256", serde_json::json!("g".repeat(64))),
            (
                "rawAnswer",
                serde_json::json!("x".repeat(MAX_RAW_ANSWER_BYTES + 1)),
            ),
        ] {
            let mut input: serde_json::Value =
                serde_json::from_slice(&valid_json("x")).expect("base submission");
            input[field] = value;
            assert!(
                NativeShadowSubmission::parse_strict(
                    &serde_json::to_vec(&input).expect("encoded invalid submission")
                )
                .is_err(),
                "accepted invalid {field}"
            );
        }

        let exact = "é".repeat(MAX_RAW_ANSWER_BYTES / 2);
        assert_eq!(exact.len(), MAX_RAW_ANSWER_BYTES);
        NativeShadowSubmission::parse_strict(&valid_json(&exact))
            .expect("limit applies to UTF-8 bytes, not characters");
    }

    #[test]
    fn node_derives_candidate_and_submission_digests_from_exact_utf8_answer() {
        let first = NativeShadowSubmission::parse_strict(&valid_json("fn answer() {}"))
            .expect("first submission");
        let second = NativeShadowSubmission::parse_strict(&valid_json("fn answer(){ }"))
            .expect("second submission");

        assert_eq!(
            first.candidate_digest_hex(),
            "9495b6a26c6523a1c29daecdc0dec4184bc199e692875e0c357077773aae2b39"
        );
        assert_ne!(first.candidate_digest_hex(), second.candidate_digest_hex());
        assert_ne!(
            first.submission_digest_hex(),
            second.submission_digest_hex()
        );
        assert_eq!(first.raw_answer_bytes(), b"fn answer() {}");
    }
}
