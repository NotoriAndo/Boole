//! A separate, non-issuable, operator-admitted problem for the existing tuple
//! projection checker. No uploaded source, executable, path or runtime policy
//! is admitted. Task and anchor bytes are generated from a bounded typed spec.
use super::*;

pub const SIGNING_DOMAIN: &[u8] = b"BOOLE-DEVELOPMENT-TUPLE-TASK-ADMISSION-V1\0";
const SCHEMA: &str = "boole.development.tuple-task-admission.v1";
const REGISTRY_VERSION: &str = "BOOLE-DEVELOPMENT-TUPLE-TASK-REGISTRY-V1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DevelopmentTaskSpec {
    pub type_name: String,
    pub field_types: Vec<String>,
    pub task_seed: String,
    pub a0: i64,
    pub mul: i64,
    pub coeffs: Vec<i64>,
}

fn domain_hash(domain: &str, parts: &[&str]) -> String {
    let mut bytes = domain.as_bytes().to_vec();
    for part in parts {
        bytes.push(0);
        bytes.extend_from_slice(part.as_bytes());
    }
    sha256_hex(&bytes)
}

impl DevelopmentTaskSpec {
    fn generate(&self) -> Result<(CanaryBindings, Vec<u8>, Vec<u8>), CanaryError> {
        let name = &self.type_name;
        if name.is_empty()
            || name.len() > 64
            || !name.as_bytes()[0].is_ascii_uppercase()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || name == "Self"
            || name == "AcfrTy"
            || self.field_types.is_empty()
            || self.field_types.len() > 8
            || self.field_types.iter().any(|t| {
                !matches!(
                    t.as_str(),
                    "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" | "bool"
                )
            })
            || !identifier(&self.task_seed)
            || self.coeffs.len() != self.field_types.len()
            || [self.a0, self.mul]
                .iter()
                .chain(&self.coeffs)
                .any(|x| !(-1_000_000..=1_000_000).contains(x))
        {
            return Err(CanaryError::Rejected("development tuple specification"));
        }
        let fields = self
            .field_types
            .iter()
            .map(|t| format!("pub {t}"))
            .collect::<Vec<_>>()
            .join(", ");
        let anchor = format!("#[derive(Clone, Copy)]\npub struct {name}({fields});\n").into_bytes();
        let anchor_digest = sha256_hex(&anchor);
        let locator = format!("development-generated/{name}:2");
        let template = domain_hash(
            "boole.native-shadow.fixture-template.v1",
            &[&anchor_digest, &locator],
        );
        // The caller's seed alone is not task identity. Commit every semantic
        // parameter before deriving the checker-compatible challenge, so a
        // changed constant cannot retain the same public submission tuple.
        let task_seed = domain_hash(
            "boole.development.tuple-task-seed.v1",
            &[&sha256_hex(&serde_json::to_vec(self)?)],
        );
        let challenge = domain_hash(
            "boole.native-shadow.fixture-challenge.v1",
            &[&task_seed, &template],
        );
        let scaffold = format!("#![allow(unused)]\nuse crate::{name} as AcfrTy;\npub fn acfr_solve(items: &[AcfrTy]) -> i64 {{\n    // <<< ACFR-PATCH-BEGIN >>>\n    todo!()\n    // <<< ACFR-PATCH-END >>>\n}}\n");
        let mut bindings = CanaryBindings::installed()?;
        let task = serde_json::to_vec(&serde_json::json!({
            "schema": "boole.native-shadow.rust-tuple-task.v1",
            "familyVersion": bindings.get("familyVersion"),
            "templateId": template, "nonIssuable": true, "edition": "2024",
            "anchor": {"path": "anchor.rs", "sha256": anchor_digest,
                "typeName": name, "semanticLocator": locator, "fieldTypes": self.field_types},
            "taskSeed": task_seed, "checkerTaskId": format!("development-{challenge}"),
            "challengeSha256": challenge,
            "constants": {"a0": self.a0, "mul": self.mul, "coeffs": self.coeffs},
            "scaffold": scaffold,
        }))?;
        for (key, value) in [
            ("templateId", template),
            ("challengeSha256", challenge),
            ("taskDigest", sha256_hex(&task)),
            ("anchorDigest", anchor_digest),
            ("registryVersion", REGISTRY_VERSION.to_string()),
        ] {
            bindings.0.insert(key.into(), value);
        }
        // An independent development registry identity; never claim that the
        // frozen production registry contains this generated task.
        bindings.0.remove("registryDigest");
        let registry = sha256_hex(&[SIGNING_DOMAIN, &serde_json::to_vec(&bindings)?].concat());
        bindings.0.insert("registryDigest".into(), registry);
        Ok((bindings, task, anchor))
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DevelopmentTaskGrant {
    schema: String,
    run_id: String,
    journal_id: String,
    epoch: u64,
    spec: DevelopmentTaskSpec,
    bindings: CanaryBindings,
    max_checker_executions: u8,
    max_redeliveries: u8,
    non_issuable: bool,
    activation_allowed: bool,
}

impl DevelopmentTaskGrant {
    pub fn one_task(
        run_id: String,
        journal_id: String,
        epoch: u64,
        spec: DevelopmentTaskSpec,
    ) -> Result<Self, CanaryError> {
        let (bindings, _, _) = spec.generate()?;
        Ok(Self {
            schema: SCHEMA.into(),
            run_id,
            journal_id,
            epoch,
            spec,
            bindings,
            max_checker_executions: 1,
            max_redeliveries: 1,
            non_issuable: true,
            activation_allowed: false,
        })
    }
}

/// Uses the common durable one-shot capability, but a distinct signature,
/// schema, registry and generated materials. `verify_grant` for the historical
/// canary continues to accept only its original installed task.
pub fn verify_grant(
    bytes: &[u8],
    signature: &[u8],
    root: &CanaryTrustRoot,
) -> Result<VerifiedCanaryGrant, CanaryError> {
    if bytes.len() > 16_384 {
        return Err(CanaryError::Rejected("development grant size"));
    }
    validate_strict_json(bytes).map_err(|_| CanaryError::Rejected("development strict JSON"))?;
    let signature =
        Signature::from_slice(signature).map_err(|_| CanaryError::Rejected("signature length"))?;
    root.0
        .verify_strict(&[SIGNING_DOMAIN, bytes].concat(), &signature)
        .map_err(|_| CanaryError::Rejected("development operator signature"))?;
    let grant: DevelopmentTaskGrant = serde_json::from_slice(bytes)?;
    let (bindings, task, anchor) = grant.spec.generate()?;
    if grant.schema != SCHEMA
        || !identifier(&grant.run_id)
        || !identifier(&grant.journal_id)
        || grant.epoch < 4
        || grant.max_checker_executions != 1
        || grant.max_redeliveries != 1
        || !grant.non_issuable
        || grant.activation_allowed
        || grant.bindings != bindings
    {
        return Err(CanaryError::Rejected(
            "development scope or material binding",
        ));
    }
    Ok(VerifiedCanaryGrant {
        grant: CanaryGrant {
            schema: grant.schema,
            run_id: grant.run_id,
            journal_id: grant.journal_id,
            epoch: grant.epoch,
            bindings,
            max_checker_executions: 1,
            max_redeliveries: 1,
            non_issuable: true,
            activation_allowed: false,
        },
        digest: sha256_hex(&[SIGNING_DOMAIN, root.0.as_bytes(), bytes].concat()),
        materials: Some(std::sync::Arc::new((task, anchor))),
        signing_domain: SIGNING_DOMAIN,
    })
}
