//! Non-issuable generated tuple adapter. This is a new, restricted answer-body
//! contract, NOT a replacement for the compiler-based Rust V1 family. No receipt,
//! signature, network, clock, process or reward authority enters this module.

use crate::{MeterError, MeterLimits, ResourceUse, TupleField, TupleItem};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_TASK_BYTES: usize = 4096;
pub const MAX_ANSWER_BYTES: usize = 8192;
const CASES: usize = 64;
const ADAPTER: &str = "BOOLE-METERED-GENERATED-TUPLE-V1";
const LIMITS: MeterLimits = MeterLimits {
    max_source_bytes: MAX_ANSWER_BYTES as u64,
    max_tokens: 512,
    max_ast_nodes: 256,
    max_ast_depth: 32,
    max_operations: 100_000,
    max_fuel: 150_000,
    max_prefix_items: 2080,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Task {
    schema: String,
    field_types: Vec<FieldType>,
    task_seed: String,
    a0: i64,
    mul: i64,
    coeffs: Vec<i64>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum FieldType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    Bool,
}

impl<'de> Deserialize<'de> for FieldType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Derived unit enums also admit {"u8": null}. This wire contract
        // deliberately permits only the documented string representation.
        let name = String::deserialize(deserializer)?;
        match name.as_str() {
            "u8" => Ok(Self::U8),
            "u16" => Ok(Self::U16),
            "u32" => Ok(Self::U32),
            "u64" => Ok(Self::U64),
            "i8" => Ok(Self::I8),
            "i16" => Ok(Self::I16),
            "i32" => Ok(Self::I32),
            "i64" => Ok(Self::I64),
            "bool" => Ok(Self::Bool),
            _ => Err(serde::de::Error::custom("unsupported tuple field type")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputError {
    TaskTooLarge,
    InvalidTaskSpecification,
    AnswerTooLarge,
}

impl std::fmt::Display for InputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for InputError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Accepted,
    DeterministicReject,
}

/// Only a complete evaluation carries counters: absence on a rejection does
/// not mean zero work. The result never attests a supply, receipt or reward.
#[derive(Debug, Clone)]
pub struct Verification {
    task_digest: String,
    answer_digest: String,
    verdict: Verdict,
    reason: String,
    resources: Option<ResourceUse>,
    corpus_digest: Option<String>,
    outputs_digest: Option<String>,
}

impl Verification {
    pub fn verdict(&self) -> Verdict {
        self.verdict
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn resource_use(&self) -> Option<ResourceUse> {
        self.resources
    }

    /// Stable JSON bytes, including every task/answer/policy input. No runtime
    /// IDs, host observations or local deadlines can change these bytes.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema": "boole.metered-tuple-verification.v1",
            "adapter": ADAPTER,
            "policyDigest": policy_digest(),
            "taskDigest": self.task_digest,
            "answerDigest": self.answer_digest,
            "verdict": self.verdict,
            "reason": self.reason,
            "resourceUse": self.resources.map(|r| hex::encode(r.canonical_bytes())),
            "corpusDigest": self.corpus_digest,
            "outputsDigest": self.outputs_digest,
            "nonIssuable": true,
            "activationAllowed": false,
        }))
        .expect("bounded primitive verification values serialize")
    }

    fn reject(mut self, reason: impl Into<String>) -> Self {
        self.verdict = Verdict::DeterministicReject;
        self.reason = reason.into();
        self
    }
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

// Length framing avoids collisions between adjacent variable-size components.
fn domain_hash(domain: &str, parts: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    digest.update([0]);
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    hex::encode(digest.finalize())
}

fn policy_digest() -> String {
    let mut bytes = Vec::new();
    for value in [
        MAX_TASK_BYTES as u64,
        CASES as u64,
        LIMITS.max_source_bytes,
        LIMITS.max_tokens,
        LIMITS.max_ast_nodes,
        LIMITS.max_ast_depth,
        LIMITS.max_operations,
        LIMITS.max_fuel,
        LIMITS.max_prefix_items,
    ] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    domain_hash(
        "boole.metered-tuple-policy.v1",
        &[ADAPTER.as_bytes(), &bytes],
    )
}

/// Exact local contract required by the non-activated package consumer. Source
/// and declared build inputs identify this implementation, not the executable
/// actually running: this is NOT a release signature or binary attestation.
pub fn contract_bytes() -> Vec<u8> {
    let implementation = domain_hash(
        "boole.metered-tuple-implementation.v1",
        &[
            include_bytes!("lib.rs"),
            include_bytes!("tuple_verifier.rs"),
            include_bytes!("../Cargo.toml"),
            include_bytes!("../../../Cargo.toml"),
            include_bytes!("../../../Cargo.lock"),
            include_bytes!("../../../rust-toolchain.toml"),
        ],
    );
    serde_json::to_vec(&serde_json::json!({
        "schema": "boole.metered-tuple-contract.v1",
        "adapter": ADAPTER,
        "implementationDigest": implementation,
        "policyDigest": policy_digest(),
        "taskSchema": "boole.metered-tuple-task.v1",
        "resultSchema": "boole.metered-tuple-verification.v1",
        "limits": {
            "taskBytes": MAX_TASK_BYTES,
            "answerBytes": LIMITS.max_source_bytes,
            "tokens": LIMITS.max_tokens,
            "astNodes": LIMITS.max_ast_nodes,
            "astDepth": LIMITS.max_ast_depth,
            "operations": LIMITS.max_operations,
            "fuel": LIMITS.max_fuel,
            "prefixItems": LIMITS.max_prefix_items,
            "cases": CASES,
        },
        "nonIssuable": true,
        "activationAllowed": false,
    }))
    .expect("constant contract serialization")
}

fn load_task(raw: &[u8]) -> Result<Task, InputError> {
    if raw.len() > MAX_TASK_BYTES {
        return Err(InputError::TaskTooLarge);
    }
    // serde's struct visitor also supports positional arrays; the public
    // task wire schema is object-only. Keep typed parsing below so duplicate
    // fields are still rejected rather than collapsed by a Value round-trip.
    if raw.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(InputError::InvalidTaskSpecification);
    }
    // Struct deserialization refuses duplicate and unknown fields, floats,
    // booleans-as-integers, trailing JSON and unsupported field types.
    let task: Task =
        serde_json::from_slice(raw).map_err(|_| InputError::InvalidTaskSpecification)?;
    if task.schema != "boole.metered-tuple-task.v1"
        || !(1..=8).contains(&task.field_types.len())
        || task.coeffs.len() != task.field_types.len()
        || task.task_seed.len() != 64
        || !task
            .task_seed
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || [task.a0, task.mul]
            .iter()
            .chain(&task.coeffs)
            .any(|n| !(-1_000_000..=1_000_000).contains(n))
    {
        return Err(InputError::InvalidTaskSpecification);
    }
    Ok(task)
}

/// Verify raw answer-body bytes against a bounded generated task, using only
/// the adapter's fixed deterministic limits. Invalid task input is a caller/
/// authority error, not a claim that a miner's answer is invalid.
pub fn verify(task_specification: &[u8], answer: &[u8]) -> Result<Verification, InputError> {
    // No partial digest/evidence for oversized bytes: callers must enforce the
    // same cap when reading, and cannot promote an unbound result to a verdict.
    if answer.len() > MAX_ANSWER_BYTES {
        return Err(InputError::AnswerTooLarge);
    }
    let task = load_task(task_specification)?;
    let task_bytes = serde_json::to_vec(&task).expect("bounded task serialization");
    let task_digest = domain_hash("boole.metered-tuple-task.v1", &[&task_bytes]);
    let mut result = Verification {
        task_digest,
        answer_digest: hash(answer),
        verdict: Verdict::Accepted,
        reason: "accepted".into(),
        resources: None,
        corpus_digest: None,
        outputs_digest: None,
    };
    let Ok(source) = std::str::from_utf8(answer) else {
        return Ok(result.reject("invalid_utf8"));
    };
    let program = match crate::parse(source, LIMITS) {
        Ok(program) => program,
        Err(error) => return Ok(result.reject(meter_reason(error))),
    };
    let seed = domain_hash(
        "boole.metered-tuple-cases.v1",
        &[
            result.task_digest.as_bytes(),
            result.answer_digest.as_bytes(),
        ],
    );
    let (items, expected, corpus_digest) = hidden_cases(&task, &seed);
    result.corpus_digest = Some(corpus_digest);
    let evaluated = match program.evaluate_hidden_prefixes(&items, LIMITS) {
        Ok(evaluated) => evaluated,
        Err(error) => return Ok(result.reject(meter_reason(error))),
    };
    result.resources = Some(evaluated.resource_use());
    result.outputs_digest = Some(hash(&i64_bytes(evaluated.outputs())));
    if evaluated.outputs() != expected {
        return Ok(result.reject("answer_mismatch"));
    }
    Ok(result)
}

fn meter_reason(error: MeterError) -> String {
    match error {
        MeterError::InvalidSyntax => "invalid_syntax".into(),
        MeterError::ForbiddenConstruct(_) => "forbidden_construct".into(),
        MeterError::BudgetExceeded(counter) => format!("budget_exceeded:{counter}"),
        MeterError::CounterOverflow => "counter_overflow".into(),
        MeterError::InvalidProgram(_) => "invalid_program".into(),
    }
}

fn i64_bytes(values: &[i64]) -> Vec<u8> {
    values.iter().flat_map(|n| n.to_be_bytes()).collect()
}

fn hidden_cases(task: &Task, seed: &str) -> (Vec<TupleItem>, Vec<i64>, String) {
    let mut state = u64::from_str_radix(&seed[..16], 16).expect("internal SHA-256 hex");
    let mut items = Vec::with_capacity(CASES);
    let mut expected = Vec::with_capacity(CASES);
    let mut corpus = Vec::new();
    let mut acc = task.a0;
    for _ in 0..CASES {
        let mut fields = Vec::with_capacity(task.field_types.len());
        let mut projection = 0i64;
        for (kind, coefficient) in task.field_types.iter().zip(&task.coeffs) {
            let random = splitmix64(&mut state);
            let (field, number) = match kind {
                FieldType::Bool => (
                    TupleField::Bool(!random.is_multiple_of(2)),
                    (random % 2) as i64,
                ),
                FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64 => {
                    let number = random % 61;
                    (TupleField::Unsigned(number), number as i64)
                }
                _ => {
                    let number = (random % 121) as i64 - 60;
                    (TupleField::Signed(number), number)
                }
            };
            fields.push(field);
            corpus.extend_from_slice(&number.to_be_bytes());
            projection = projection.wrapping_add(number.wrapping_mul(*coefficient));
        }
        acc = acc.wrapping_mul(task.mul).wrapping_add(projection);
        items.push(TupleItem::new(fields));
        expected.push(acc);
    }
    corpus.extend_from_slice(&i64_bytes(&expected));
    (items, expected, hash(&corpus))
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
