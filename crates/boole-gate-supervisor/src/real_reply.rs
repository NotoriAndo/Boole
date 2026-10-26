//! Strict real-release final channel; package allowances are caller authenticated.
use crate::real_descriptor::{ascii, hash, Descriptor};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub chain_id: u64,
    pub problem_id: String,
    pub commit_hash: String,
    pub solver: String,
    pub env_id: String,
    pub export_digest: String,
    pub package_sha256: String,
    pub leaf_kind: u8,
    pub lane: u8,
    pub slot_fuel: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kernel {
    pub state: String,
    pub debit: Option<u64>,
    pub charged: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kernels {
    pub lean: Kernel,
    pub nanoda: Kernel,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub version: u64,
    pub gate_rule_digest: String,
    pub descriptor_sha256: String,
    pub input_digest: String,
    pub outcome: String,
    pub reason: u16,
    pub action: String,
    pub failure_class: String,
    pub detail: String,
    pub gate_error: String,
    pub row: String,
    pub native_followup: String,
    pub statement_id: String,
    pub fuel: Option<u64>,
    pub kernels: Kernels,
    pub ext: Vec<u8>,
    pub assurance: String,
    pub counter_identity: Option<Counters>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counters {
    pub lean: Counter<Lean>,
    pub nanoda: Counter<Nanoda>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counter<T> {
    pub summary: T,
    pub vector: Vector,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vector {
    pub sha256: String,
    pub records: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lean {
    pub calls: u64,
    pub checked: u64,
    pub code: u64,
    pub debit: u64,
    pub depth: u64,
    pub version: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Nanoda {
    pub version: u64,
    pub code: u64,
    pub checked_declarations: u64,
    pub aggregate_work: u64,
    pub aggregate_steps: u64,
    pub maximum_depth: u64,
    pub maximum_composed_depth: u64,
    pub selected_declarations: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caps {
    pub lean: u64,
    pub nanoda: u64,
    pub statement: String,
}
/// Only completed package authentication can select the quarantine fallback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthenticatedStatement {
    Unknown,
    Valid(String),
    NoValidStatement,
}

fn hex(bytes: &[u8]) -> String {
    boole_derivation::attributes::hex_string(bytes)[2..].into()
}
impl Context {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.chain_id > 0
                && self.slot_fuel > 0
                && [1, 2].contains(&self.lane)
                && [1, 2].contains(&self.leaf_kind),
            "input context"
        );
        for value in [
            &self.problem_id,
            &self.commit_hash,
            &self.env_id,
            &self.export_digest,
            &self.package_sha256,
        ] {
            hash(value)?;
        }
        ensure!(
            self.solver.len() == 40
                && self
                    .solver
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "solver encoding"
        );
        Ok(())
    }
    pub fn digest(&self) -> Result<[u8; 32]> {
        self.validate()?;
        Ok(crate::codec::digest(&ascii(self)?))
    }
    pub fn void_statement(&self) -> Result<String> {
        self.validate()?;
        let mut b = b"BOOLE-M3/VOID-STATEMENT/v1\0".to_vec();
        b.extend(self.chain_id.to_be_bytes());
        for value in [&self.problem_id, &self.env_id, &self.package_sha256] {
            b.extend(hash(value)?);
        }
        b.extend(0x0801u16.to_be_bytes());
        Ok(hex(&crate::codec::digest(&b)))
    }
}
// Exact M3d-a closed rows, including deliberate native-cause gaps. No diagnostic
// text or exit status can introduce an additional reason into this table.
struct Row {
    outcome: &'static str,
    reason: u16,
    action: &'static str,
    class: &'static str,
    error: &'static str,
    followup: &'static str,
}
fn row(name: &str, detail: &str) -> Result<Row> {
    let (outcome, reason, action, class, error, followup) = match name {
        "accept" => ("ACCEPT", 0, "apply", "", "", ""),
        "front_invalid" | "unknown_constant" => ("REJECT", 0x0106, "apply", "", "", ""),
        "name_grammar" => ("REJECT", 0x0103, "apply", "", "", ""),
        "structural_cap" => ("REJECT", 0x0108, "apply", "", "", ""),
        "reserved_name" => ("REJECT", 0x0201, "apply", "", "", ""),
        "statement_mismatch" => ("REJECT", 0x0107, "apply", "", "", ""),
        "axiom_rejected" => ("REJECT", 0x0305, "apply", "", "", ""),
        "kernel_reject" | "input_type" => ("REJECT", 0x0301, "apply", "", "", ""),
        "kernel_disagreement" => ("REJECT", 0x0302, "apply", "", "", ""),
        "resource_cap" => ("REJECT", 0x0303, "apply", "", "", ""),
        "depth_cause_missing" => (
            "DEFER",
            0,
            "repair",
            "D-node",
            "environment-native-cause-unavailable",
            "distinct-depth-cause",
        ),
        "cap_cause_missing" => (
            "DEFER",
            0,
            "repair",
            "D-node",
            "environment-native-cause-unavailable",
            "typed-cap-origin",
        ),
        "kernel_panic_cause_missing" => (
            "DEFER",
            0,
            "repair",
            "D-node",
            "environment-native-cause-unavailable",
            "explicit-kernel-panic-cause",
        ),
        "output_terminal" => ("REJECT", 0x0109, "terminal", "S", "output-cap", ""),
        "library_terminal" => (
            "REJECT",
            0x010a,
            "terminal",
            "S",
            "library-outside-certified-base",
            "",
        ),
        "transient" => ("DEFER", 0, "retry", "T", "resource-unavailable", ""),
        "input_failure" => (
            "DEFER",
            0,
            "retry",
            if detail == "malformed-record" {
                "I"
            } else {
                "D-input"
            },
            "replay-unavailable",
            "",
        ),
        "node_fault" => (
            "DEFER",
            0,
            "repair",
            "D-node",
            "environment-tool-unavailable",
            "",
        ),
        "input_availability" => ("STALL", 0, "wait", "", "", ""),
        "input_integrity" => (
            "STOP",
            0,
            "preserve",
            if detail == "malformed-record" {
                "I"
            } else {
                "D-input"
            },
            "replay-unavailable",
            "",
        ),
        "package_invalid" => ("VOID", 0x0801, "apply", "", "", ""),
        "fuel_binding_invalid" => ("VOID", 0x0803, "apply", "", "", ""),
        "generator_unavailable" => ("VOID", 0x0804, "apply", "", "", ""),
        "malformed_result" => (
            "DEFER",
            0,
            "repair",
            "D-node",
            "environment-result-unavailable",
            "",
        ),
        "trusted_disagreement" => ("DEFER", 0, "retry", "I", "replay-unavailable", ""),
        "quarantine_void" => ("VOID", 0x0802, "fork-only", "", "", ""),
        _ => anyhow::bail!("unknown real reply row"),
    };
    Ok(Row {
        outcome,
        reason,
        action,
        class,
        error,
        followup,
    })
}
impl Reply {
    pub fn decode(
        raw: &[u8],
        d: &Descriptor,
        context: &Context,
        caps: &Caps,
        fork: bool,
    ) -> Result<Self> {
        let auth = if caps.statement == "0".repeat(64) {
            AuthenticatedStatement::Unknown
        } else {
            AuthenticatedStatement::Valid(caps.statement.clone())
        };
        Self::decode_with_statement(raw, d, context, caps, fork, &auth)
    }
    pub fn decode_with_statement(
        raw: &[u8],
        d: &Descriptor,
        context: &Context,
        caps: &Caps,
        fork: bool,
        authenticated: &AuthenticatedStatement,
    ) -> Result<Self> {
        ensure!(raw.len() <= 65536, "reply byte cap");
        context.validate()?;
        let v: Self = serde_json::from_slice(raw)?;
        ensure!(
            ascii(&v)? == raw && v.version == 1,
            "canonical reply/version"
        );
        ensure!(
            hash(&v.gate_rule_digest)? == d.digest()
                && hash(&v.descriptor_sha256)? == crate::codec::digest(&ascii(d)?),
            "reply release mismatch"
        );
        ensure!(
            hash(&v.input_digest)? == context.digest()?,
            "reply input mismatch"
        );
        ensure!(
            v.detail.is_empty() || v.detail == "malformed-record",
            "detail enum"
        );
        let expected = row(&v.row, &v.detail)?;
        ensure!(
            v.outcome == expected.outcome
                && v.reason == expected.reason
                && v.action == expected.action
                && v.failure_class == expected.class
                && v.gate_error == expected.error
                && v.native_followup == expected.followup,
            "closed reply mapping"
        );
        ensure!(
            v.row != "trusted_disagreement" || v.detail == "malformed-record",
            "disagreement detail"
        );
        ensure!(
            v.row != "quarantine_void" || fork,
            "published fork authorization"
        );
        let fingerprint = context.void_statement()?;
        let quarantine = context.quarantine_statement()?;
        if v.reason == 0x0802 && v.outcome == "VOID" {
            ensure!(fork, "fork-only quarantine");
            let expected = match authenticated {
                AuthenticatedStatement::Valid(value) => {
                    hash(value)?;
                    ensure!(
                        *value != quarantine && *value != fingerprint,
                        "fingerprint is not an authenticated proof statement"
                    );
                    value.clone()
                }
                AuthenticatedStatement::NoValidStatement => quarantine.clone(),
                AuthenticatedStatement::Unknown => {
                    anyhow::bail!("quarantine statement not authenticated")
                }
            };
            ensure!(
                v.statement_id == expected && v.statement_id != fingerprint,
                "quarantine statement binding"
            );
        } else if v.reason == 0x0801 && v.outcome == "VOID" {
            ensure!(v.statement_id == fingerprint, "VOID-only fingerprint");
        } else {
            ensure!(
                v.statement_id != quarantine,
                "quarantine fingerprint outside 0802"
            );
            ensure!(
                v.statement_id != fingerprint,
                "VOID fingerprint outside 0801"
            );
            if v.statement_id != "0".repeat(64) {
                hash(&v.statement_id)?;
            } else {
                ensure!(
                    v.outcome != "ACCEPT" && v.outcome != "VOID",
                    "nonzero statement"
                );
            }
            if ["ACCEPT", "REJECT", "VOID"].contains(&v.outcome.as_str()) {
                ensure!(
                    v.statement_id == caps.statement,
                    "package statement binding"
                );
            }
        }
        let intake = v.row == "output_terminal";
        let mut total = 0u64;
        let mut incomplete = false;
        let mut exceeded = false;
        let mut executed = false;
        for (state, cap, weight) in [
            (&v.kernels.lean, caps.lean, d.fuel.lean_weight),
            (&v.kernels.nanoda, caps.nanoda, d.fuel.nanoda_weight),
        ] {
            match state.state.as_str() {
                "unknown" => {
                    ensure!(state.debit.is_none(), "fabricated unknown debit");
                    if intake {
                        ensure!(state.charged == Some(0), "terminal intake charge");
                    } else {
                        ensure!(state.charged.is_none(), "fabricated unknown charge");
                        incomplete = true;
                    }
                    continue;
                }
                "not-run" => {
                    ensure!(
                        state.debit.is_none() && state.charged == Some(0) && !intake,
                        "fabricated not-run counter"
                    );
                }
                "executed" => {
                    executed = true;
                    ensure!(!intake && cap > 0, "executed kernel allowance");
                    let debit = state
                        .debit
                        .ok_or_else(|| anyhow::anyhow!("missing executed debit"))?;
                    let expected = if v.row == "resource_cap" {
                        debit.min(cap)
                    } else {
                        ensure!(debit <= cap, "package kernel cap");
                        debit
                    };
                    ensure!(
                        state.charged == Some(expected),
                        "exact bounded debit charge"
                    );
                    exceeded |= debit > cap;
                }
                _ => anyhow::bail!("kernel state enum"),
            }
            let charge = state
                .charged
                .ok_or_else(|| anyhow::anyhow!("missing kernel charge"))?;
            total = total
                .checked_add(
                    charge
                        .checked_mul(weight)
                        .ok_or_else(|| anyhow::anyhow!("fuel multiplication overflow"))?,
                )
                .ok_or_else(|| anyhow::anyhow!("fuel sum overflow"))?;
        }
        if incomplete {
            ensure!(
                v.fuel.is_none() && v.outcome == "DEFER",
                "missing executed work"
            );
        } else {
            ensure!(
                v.fuel == Some(total) && total <= context.slot_fuel,
                "scalar fuel/slot allowance"
            );
        }
        ensure!(
            v.row != "resource_cap" || !executed || exceeded,
            "aggregate cap cause missing"
        );
        if v.outcome == "ACCEPT"
            || ["kernel_reject", "kernel_disagreement"].contains(&v.row.as_str())
        {
            ensure!(
                v.kernels.lean.state == "executed" && v.kernels.nanoda.state == "executed",
                "both executed kernels required"
            );
        }
        if v.outcome == "ACCEPT" {
            ensure!(context.lane == 1, "lane-2 unqualified");
        }
        if v.outcome == "VOID" {
            ensure!(
                v.kernels.lean.state == "not-run"
                    && v.kernels.nanoda.state == "not-run"
                    && v.fuel == Some(0),
                "VOID before kernels"
            );
        }
        ensure!(
            v.ext.is_empty() && v.assurance == "development-only",
            "A1 assurance/ext"
        );
        if let Some(counter) = &v.counter_identity {
            for (version, code, vector) in [
                (
                    counter.lean.summary.version,
                    counter.lean.summary.code,
                    &counter.lean.vector,
                ),
                (
                    counter.nanoda.summary.version,
                    counter.nanoda.summary.code,
                    &counter.nanoda.vector,
                ),
            ] {
                ensure!(
                    version == 1 && [0, 3].contains(&code),
                    "counter identity version/code"
                );
                hash(&vector.sha256)?;
            }
        }
        Ok(v)
    }
}
impl Context {
    pub fn from_input(i: &crate::GateInput) -> Self {
        Self {
            chain_id: i.chain_id,
            problem_id: hex(&i.problem_id),
            commit_hash: hex(&i.commit_hash),
            solver: hex(&i.solver),
            env_id: hex(&i.env_id),
            export_digest: hex(&i.export_digest),
            package_sha256: hex(&crate::codec::digest(&i.package)),
            leaf_kind: i.leaf_kind,
            lane: i.lane,
            slot_fuel: i.fuel_slot,
        }
    }
}
impl Reply {
    /// Trusted reconstruction/fork failures use only existing closed real rows.
    /// No timeout, diagnostic, artifact fault or retry exhaustion calls this.
    pub fn reconstructed(
        name: &str,
        d: &Descriptor,
        c: &Context,
        caps: &Caps,
        fork: bool,
    ) -> Result<Vec<u8>> {
        let auth = if caps.statement == "0".repeat(64) {
            AuthenticatedStatement::Unknown
        } else {
            AuthenticatedStatement::Valid(caps.statement.clone())
        };
        Self::reconstructed_with_statement(name, d, c, caps, fork, &auth)
    }
    pub fn reconstructed_with_statement(
        name: &str,
        d: &Descriptor,
        c: &Context,
        caps: &Caps,
        fork: bool,
        authenticated: &AuthenticatedStatement,
    ) -> Result<Vec<u8>> {
        ensure!(
            [
                "front_invalid",
                "package_invalid",
                "quarantine_void",
                "fuel_binding_invalid",
                "generator_unavailable",
                "node_fault",
                "input_integrity",
                "structural_cap"
            ]
            .contains(&name),
            "local real row"
        );
        let r = row(name, "")?;
        let statement = if name == "package_invalid" {
            c.void_statement()?
        } else if name == "quarantine_void" {
            match authenticated {
                AuthenticatedStatement::Valid(value) => value.clone(),
                AuthenticatedStatement::NoValidStatement => c.quarantine_statement()?,
                AuthenticatedStatement::Unknown => {
                    anyhow::bail!("quarantine statement not authenticated")
                }
            }
        } else {
            caps.statement.clone()
        };
        let not_run = || Kernel {
            state: "not-run".into(),
            debit: None,
            charged: Some(0),
        };
        let value = Self {
            version: 1,
            gate_rule_digest: hex(&d.digest()),
            descriptor_sha256: hex(&crate::codec::digest(&ascii(d)?)),
            input_digest: hex(&c.digest()?),
            outcome: r.outcome.into(),
            reason: r.reason,
            action: r.action.into(),
            failure_class: r.class.into(),
            detail: String::new(),
            gate_error: r.error.into(),
            row: name.into(),
            native_followup: r.followup.into(),
            statement_id: statement,
            fuel: Some(0),
            kernels: Kernels {
                lean: not_run(),
                nanoda: not_run(),
            },
            ext: vec![],
            assurance: "development-only".into(),
            counter_identity: None,
        };
        let raw = ascii(&value)?;
        Self::decode_with_statement(&raw, d, c, caps, fork, authenticated)?;
        Ok(raw)
    }
}

impl Context {
    pub fn input(&self, raw: Vec<u8>, package: Vec<u8>) -> Result<crate::GateInput> {
        self.validate()?;
        let mut solver = [0; 20];
        for (i, b) in solver.iter_mut().enumerate() {
            *b = u8::from_str_radix(&self.solver[i * 2..i * 2 + 2], 16)?;
        }
        ensure!(
            crate::codec::digest(&raw) == hash(&self.export_digest)?
                && crate::codec::digest(&package) == hash(&self.package_sha256)?,
            "real context content addresses"
        );
        Ok(crate::GateInput {
            chain_id: self.chain_id,
            problem_id: hash(&self.problem_id)?,
            commit_hash: hash(&self.commit_hash)?,
            solver,
            env_id: hash(&self.env_id)?,
            export_digest: hash(&self.export_digest)?,
            lane: self.lane,
            leaf_kind: self.leaf_kind,
            instance_seed: [0; 32],
            fuel_slot: self.slot_fuel,
            raw,
            package,
            artifacts: vec![],
            early: None,
        })
    }
}

impl Context {
    /// Own authenticated transport has not produced decoded bytes yet.
    pub fn carried_input(&self, package: Vec<u8>) -> Result<crate::GateInput> {
        self.validate()?;
        ensure!(
            crate::codec::digest(&package) == hash(&self.package_sha256)?,
            "carried package address"
        );
        let mut context = self.clone();
        context.export_digest = hex(&crate::codec::digest(&[]));
        let mut input = context.input(vec![], package)?;
        input.export_digest = hash(&self.export_digest)?;
        Ok(input)
    }
}

impl Context {
    pub fn quarantine_statement(&self) -> Result<String> {
        self.validate()?;
        let mut b = b"BOOLE-M3/QUARANTINE-STATEMENT/v1\0".to_vec();
        b.extend(self.chain_id.to_be_bytes());
        for value in [
            &self.problem_id,
            &self.commit_hash,
            &self.env_id,
            &self.package_sha256,
        ] {
            b.extend(hash(value)?);
        }
        b.extend(0x0802u16.to_be_bytes());
        Ok(hex(&crate::codec::digest(&b)))
    }
}
