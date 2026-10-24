//! Unsigned, source-preserving devnet fold and ledger diagnostics.
use anyhow::Result;
use boole_derivation::{m2::Config, m3::ForkTable, ParentHeader};
use boole_devnet_archive::input::InputRow;
use boole_gate_supervisor::{rebuild::History, supervisor::Supervisor};
use serde_json::{json, Value};
pub trait Inputs: History {
    fn tip(&self) -> u64;
    fn row(&mut self, height: u64) -> Result<InputRow>;
    fn check(&self) -> Result<()>;
    fn label(&self) -> &'static str;
}
pub trait Node {
    fn head(&mut self) -> Result<ParentHeader>;
    fn header(&mut self, height: u64) -> Result<ParentHeader>;
    fn transactions(&mut self, height: u64) -> Result<Vec<String>>;
    fn receipt_statuses(&mut self, height: u64) -> Result<Vec<bool>>;
    fn depositor_balance(&mut self, height: u64) -> Result<u128>;
}
pub struct ExpectedHeads {
    pub l1_number: u64,
    pub l1_hash: String,
    pub l2: Option<(u64, String)>,
}
#[derive(Debug)]
struct Mismatch {
    height: u64,
    class: &'static str,
}
fn at<T>(height: u64, class: &'static str, result: Result<T>) -> std::result::Result<T, Mismatch> {
    result.map_err(|_| Mismatch { height, class })
}
fn require(height: u64, class: &'static str, condition: bool) -> std::result::Result<(), Mismatch> {
    if condition {
        Ok(())
    } else {
        Err(Mismatch { height, class })
    }
}
fn input_items(logs: &[boole_derivation::InboxLog]) -> Result<Vec<boole_derivation::m2::Input>> {
    let mut out = vec![];
    for log in logs {
        let bytes = boole_derivation::attributes::hex_bytes(&log.data, "item")?;
        if let Ok((kind, body)) = boole_derivation::inbox::item(&bytes) {
            if (2..=15).contains(&kind) {
                out.push(boole_derivation::m2::Input {
                    kind,
                    body: body.to_vec(),
                    transaction_index: log.transaction_index,
                    log_index: log.log_index,
                });
            }
        }
    }
    Ok(out)
}
fn fork_digest(forks: Option<&ForkTable>) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(b"BOOLE-M4/AUDIT-FORKS/v1");
    let rows = forks.map(|f| f.rows.as_slice()).unwrap_or(&[]);
    digest.update(u32::try_from(rows.len())?.to_be_bytes());
    for row in rows {
        digest.update(row.height.to_be_bytes());
        digest.update(row.activation_record()?);
    }
    Ok(boole_derivation::attributes::hex_string(&digest.finalize()))
}
/// Check published activation bytes and the activation-driven queue order.
pub fn verify_fork_events(
    forks: &ForkTable,
    before: &boole_derivation::m2::State,
    after: &boole_derivation::m2::State,
    height: u64,
    raw: &[u8],
) -> Result<()> {
    use anyhow::ensure;
    use boole_derivation::m3::{ForkKind, RecordKind};
    forks.validate_history(after)?;
    let activated = forks.rows.iter().skip(1).find(|r| r.height == height);
    let activation: Vec<_> = ledger::events(raw)?
        .into_iter()
        .filter(|(tag, _)| *tag == 8)
        .collect();
    ensure!(
        activation.len() == usize::from(activated.is_some()),
        "activation count"
    );
    if let Some(row) = activated {
        ensure!(
            activation[0].1 == row.activation_record()?,
            "published activation row"
        );
        if row.kind == ForkKind::KernelFix {
            let expected = forks.recheck_set(before, height)?;
            let start = before
                .m3
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("M3 queue"))?
                .queue
                .len();
            let queue = &after
                .m3
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("M3 queue"))?
                .queue;
            let actual: Vec<_> = queue
                .get(start..)
                .ok_or_else(|| anyhow::anyhow!("queue truncated"))?
                .iter()
                .filter(|j| j.kind == RecordKind::Recheck && j.epoch_override == Some(row.epoch))
                .map(|j| j.key)
                .collect();
            ensure!(actual == expected, "kernel-fix recheck order");
        }
    }
    Ok(())
}
fn counts(state: &boole_derivation::m2::State) -> Value {
    let mut status = std::collections::BTreeMap::<String, u64>::new();
    for candidate in state.candidates.values() {
        *status
            .entry(format!("{:?}", candidate.status()).to_uppercase())
            .or_default() += 1;
    }
    let mut reasons = std::collections::BTreeMap::<String, u64>::new();
    if let Some(m) = &state.m3 {
        for record in m.results.values() {
            *reasons
                .entry(if record.reason == 0 {
                    "accept".into()
                } else {
                    format!("0x{:02x}xx", record.reason >> 8)
                })
                .or_default() += 1;
        }
    }
    let decimal = |values: std::collections::BTreeMap<String, u64>| {
        values
            .into_iter()
            .map(|(k, v)| (k, v.to_string()))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    json!({"status":decimal(status),"reason_ranges":decimal(reasons)})
}
pub fn failed_receipt(c: &Config, forks: Option<&ForkTable>, height: u64, class: &str) -> Value {
    let mut value = empty_receipt(c, "pending");
    value["fork_table_digest"] = json!(fork_digest(forks).ok());
    mark_mismatch(&mut value, height, class);
    value
}
/// Preserve the earliest mismatch while making every failed check explicit.
pub fn mark_mismatch(receipt: &mut Value, height: u64, class: &str) {
    if receipt["first_mismatch"].is_null() {
        receipt["first_mismatch"] = json!({"height":height.to_string(),"class":class});
    }
    receipt["checks"][class] = json!("mismatch");
}
fn empty_receipt(c: &Config, label: &str) -> Value {
    json!({"schema":"boole.devnet.audit.v1","unsigned":true,"mode":"pure","config_digest":boole_derivation::attributes::hex_string(&c.digest()),"fork_table_digest":null,"anchor":null,"l1_head":null,"l2_head":null,"root":null,"audited_through":"0","observed_l2_head":null,"labels":{"inputs":label,"verdicts":"computed","environment":"pending","ranges":[]},"counts":{"status":{},"reason_ranges":{}},"rewards":ledger::Ledger::default().totals(),"admissions":{"total":"0","active":"0","retired":"0"},"environment_hash":null,"environment":{"status":"pending","reason":"J4 environment build and normalized identity deferred"},"k4":{"status":"pending","checked":"0","total":"0","pin_retention":"pending-M4c"},"checks":{"input":"pending","fold":"pending","prefix":"pending","verdict":"pending","ledger":"pending","admission":"pending","forks":"pending","execution":"not-run"},"first_mismatch":null})
}
/// Read-only pure audit. Computed verdicts come only from the pinned supervisor;
/// the audited node supplies bytes to compare, never a substitute fold result.
pub fn audit(
    inputs: &mut dyn Inputs,
    node: &mut dyn Node,
    c: &Config,
    forks: Option<&ForkTable>,
    supervisor: Option<&mut Supervisor>,
    expected: &ExpectedHeads,
) -> Value {
    match boole_devnet_archive::durability::PrivateTempDir::new("devnet-audit-pure") {
        Ok(work) => audit_in(inputs, node, c, forks, supervisor, expected, work.path()),
        Err(_) => failed_receipt(c, forks, 0, "work-dir"),
    }
}
pub fn audit_in(
    inputs: &mut dyn Inputs,
    node: &mut dyn Node,
    c: &Config,
    forks: Option<&ForkTable>,
    mut supervisor: Option<&mut Supervisor>,
    expected: &ExpectedHeads,
    work: &std::path::Path,
) -> Value {
    use boole_derivation::{
        attributes::{derive_with_m2, derive_with_m3, hex_string},
        m2::State,
    };
    let tip = inputs.tip();
    let mut receipt = empty_receipt(c, inputs.label());
    let result = (|| -> std::result::Result<(), Mismatch> {
        at(0, "input", inputs.check())?;
        let genesis = at(0, "input", inputs.row(0))?;
        let anchor = at(0, "input", genesis.verify())?;
        require(0, "input", anchor.block.number == 0)?;
        receipt["anchor"] = json!({"number":"0","hash":anchor.block.hash});
        require(expected.l1_number, "input", expected.l1_number <= tip)?;
        let external = at(
            expected.l1_number,
            "input",
            inputs.row(expected.l1_number).and_then(|r| r.verify()),
        )?;
        require(
            expected.l1_number,
            "input",
            external.block.hash.eq_ignore_ascii_case(&expected.l1_hash),
        )?;
        let head = at(tip, "prefix", node.head())?;
        require(tip, "prefix", head.number == tip)?;
        if let Some((h, hash)) = &expected.l2 {
            let block = at(*h, "prefix", node.header(*h))?;
            require(*h, "prefix", block.hash.eq_ignore_ascii_case(hash))?;
        }
        receipt["observed_l2_head"] = json!({"number":head.number.to_string(),"hash":head.hash});
        let mut previous = at(0, "prefix", node.header(0))?;
        receipt["l2_head"] = json!({"number":"0","hash":previous.hash});
        receipt["l1_head"] = json!({"number":"0","hash":anchor.block.hash});
        let mut state = at(
            0,
            "fold",
            State::new(c, previous.gas_limit).map_err(Into::into),
        )?;
        if let Some(f) = forks {
            at(
                0,
                "fold",
                f.validate(c).map_err(|s| anyhow::anyhow!("S{s}")),
            )?;
        }
        receipt["fork_table_digest"] = json!(at(0, "fold", fork_digest(forks))?);
        let mut ledger = ledger::Ledger::default();
        let mut record_heights = std::collections::BTreeMap::new();
        let mut last_l1 = anchor.block;
        for h in 1..=tip {
            at(h, "input", inputs.check())?;
            let row = at(h, "input", inputs.row(h))?;
            let verified = at(h, "input", row.verify())?;
            let l1 = verified.block;
            require(h, "input", l1.number == h && l1.parent_hash == last_l1.hash)?;
            let block = at(h, "prefix", node.header(h))?;
            require(
                h,
                "prefix",
                block.number == h && block.parent_hash.as_deref() == Some(previous.hash.as_str()),
            )?;
            let actual = at(h, "prefix", node.transactions(h))?;
            at(h, "input", inputs.check())?;
            let items = at(h, "fold", input_items(&verified.logs))?;
            let (derived, next, events, payments) = if c.m3.is_some() {
                let f = forks.ok_or(Mismatch {
                    height: h,
                    class: "fold",
                })?;
                let runtime = supervisor.as_deref_mut().ok_or(Mismatch {
                    height: h,
                    class: "verdict",
                })?;
                let records = at(h, "verdict", runtime.collect(&state, c, h, f, inputs))?;
                let step = at(
                    h,
                    "fold",
                    state
                        .step_with_verdicts(c, &l1, &items, &records, f)
                        .map_err(Into::into),
                )?;
                let (d, n) = at(
                    h,
                    "fold",
                    derive_with_m3(&state, c, &l1, &verified.logs, &previous, &records, f)
                        .map_err(Into::into),
                )?;
                require(h, "fold", step.state.root == n.root)?;
                (d, n, step.events, step.payments)
            } else {
                let step = at(h, "fold", state.step(c, &l1, &items).map_err(Into::into))?;
                let (d, n) = at(
                    h,
                    "fold",
                    derive_with_m2(&state, c, &l1, &verified.logs, &previous).map_err(Into::into),
                )?;
                (d, n, step.events, step.payments)
            };
            require(
                h,
                "prefix",
                format!("0x{:x}", block.timestamp) == derived.attributes.timestamp
                    && format!("0x{:x}", block.gas_limit) == derived.attributes.gas_limit,
            )?;
            if let Some(after) = &next.m3 {
                for (key, record) in &after.results {
                    if state.m3.as_ref().and_then(|m| m.results.get(key)) != Some(record) {
                        record_heights.insert(*key, h);
                    }
                }
            }
            at(
                h,
                "prefix",
                boole_devnet_driver::driver::check_payload_transactions(
                    &json!({"transactions":actual}),
                    &derived,
                ),
            )?;
            at(
                h,
                "prefix",
                boole_derivation::system::check_m2_root(
                    block.m2_root_deposit.as_deref(),
                    h,
                    &l1.hash,
                    next.root,
                )
                .map_err(Into::into),
            )?;
            if let Some(f) = forks {
                at(h, "fold", verify_fork_events(f, &state, &next, h, &events))?;
            }
            let statuses = at(h, "ledger", node.receipt_statuses(h))?;
            let ledger_result = ledger.observe(
                &state, &next, c, &l1, &events, &payments, &actual, &statuses,
            );
            let class = if ledger_result
                .as_ref()
                .err()
                .is_some_and(|e| e.is::<ledger::AdmissionViolation>())
            {
                "admission"
            } else {
                "ledger"
            };
            at(h, class, ledger_result)?;
            let balance = at(h, "ledger", node.depositor_balance(h))?;
            require(h, "ledger", balance == ledger.failed_mints())?;
            at(h, "input", inputs.check())?;
            receipt["l1_head"] = json!({"number":h.to_string(),"hash":l1.hash});
            receipt["root"] = json!(hex_string(&next.root));
            receipt["rewards"] = ledger.totals();
            receipt["rewards"]["depositor_balance"] = json!(balance.to_string());
            receipt["admissions"] = ledger.admission_counts();
            receipt["audited_through"] = json!(h.to_string());
            receipt["l2_head"] = json!({"number":h.to_string(),"hash":block.hash});
            receipt["counts"] = counts(&next);
            last_l1 = l1;
            state = next;
            previous = block;
        }
        match k4::check(
            &state,
            c,
            forks,
            inputs,
            supervisor.as_deref().and_then(|s| s.codec.as_ref()),
            &record_heights,
            work,
        ) {
            Ok(coverage) => {
                receipt["k4"]["total"] = json!(coverage.total.to_string());
                receipt["k4"]["checked"] = json!(coverage.checked.to_string());
            }
            Err(failure) => {
                receipt["k4"]["total"] = json!(failure.coverage.total.to_string());
                receipt["k4"]["checked"] = json!(failure.coverage.checked.to_string());
                return Err(Mismatch {
                    height: failure.height,
                    class: "K4",
                });
            }
        }
        if tip == 0 {
            receipt["l1_head"] = json!({"number":"0","hash":last_l1.hash});
            receipt["root"] = json!(hex_string(&state.root));
        }
        for class in [
            "input",
            "fold",
            "prefix",
            "verdict",
            "ledger",
            "admission",
            "forks",
        ] {
            receipt["checks"][class] = json!(if c.m3.is_none()
                && matches!(class, "verdict" | "admission" | "forks")
            {
                "not-applicable"
            } else {
                "ok"
            });
        }
        receipt["labels"]["ranges"] = if tip == 0 {
            json!([])
        } else {
            json!([{"from":"1","through":tip.to_string(),"inputs":inputs.label(),"verdicts":"computed","environment":"pending"}])
        };
        at(tip, "input", inputs.check())?;
        Ok(())
    })();
    if let Err(m) = result {
        mark_mismatch(&mut receipt, m.height, m.class);
    }
    receipt
}
pub mod ledger;

pub struct JournalInputs(pub boole_devnet_archive::journal::ReadOnlyJournal);
impl Inputs for JournalInputs {
    fn tip(&self) -> u64 {
        self.0.tip()
    }
    fn row(&mut self, h: u64) -> Result<InputRow> {
        self.0.row(h)
    }
    fn check(&self) -> Result<()> {
        self.0.check()
    }
    fn label(&self) -> &'static str {
        "journal"
    }
}
impl History for JournalInputs {
    fn origin(
        &mut self,
        h: u64,
    ) -> Result<(boole_derivation::L1Block, Vec<boole_derivation::InboxLog>)> {
        let input = self.0.row(h)?.verify()?;
        Ok((input.block, input.logs))
    }
}
pub mod rpc;

pub struct ArchiveInputs(pub boole_devnet_archive::journal::ReadOnlyJournal);
impl Inputs for ArchiveInputs {
    fn tip(&self) -> u64 {
        self.0.tip()
    }
    fn row(&mut self, h: u64) -> Result<InputRow> {
        self.0.row(h)
    }
    fn check(&self) -> Result<()> {
        self.0.check()
    }
    fn label(&self) -> &'static str {
        "archive-verified"
    }
}
impl History for ArchiveInputs {
    fn origin(
        &mut self,
        h: u64,
    ) -> Result<(boole_derivation::L1Block, Vec<boole_derivation::InboxLog>)> {
        let input = self.0.row(h)?.verify()?;
        Ok((input.block, input.logs))
    }
}
pub mod replay;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fork_table_digest_binds_activation_heights_not_only_record_bodies() -> Result<()> {
        let tape: Value = serde_json::from_str(include_str!(
            "../../../fixtures/devnet-m3/kernel-fix-v1.3.json"
        ))?;
        let config: Config = serde_json::from_str(tape["config_json"].as_str().unwrap())?;
        let table: ForkTable = serde_json::from_value(tape["forks"].clone())?;
        table.validate(&config).unwrap();
        let mut moved = table.clone();
        moved.rows[1].height += 1;
        moved.validate(&config).unwrap();
        assert_eq!(
            table.rows[1].activation_record()?,
            moved.rows[1].activation_record()?
        );
        assert_ne!(
            fork_digest(Some(&table))?,
            fork_digest(Some(&moved))?,
            "different activation schedules shared the audit FORK_TABLE digest"
        );
        Ok(())
    }
}

pub mod k4;
