//! Real-release branch only; the legacy stub loop and retry policy stay intact.
use super::*;
use crate::{real_descriptor::hash, real_reply::Reply};
impl Supervisor {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn collect_real_job(
        &mut self,
        s: &State,
        c: &Config,
        job: &Job,
        epoch: u32,
        forced: bool,
        key: &CacheKey,
        history: &mut dyn History,
        now: u64,
    ) -> Result<Option<VerdictRecord>> {
        let retry_key = (key.clone(), epoch);
        if self
            .real_retries
            .get(&retry_key)
            .is_some_and(|r| !r.ready(now))
        {
            return Ok(None);
        }
        if !self.canary_ok {
            self.real_retries.entry(retry_key).or_default().d_node(now);
            self.alerts
                .push("d-node: real canary unavailable; attempt not counted".into());
            return Ok(None);
        }
        let deadline = crate::real_deadline::Deadline::for_envelope(&self.envelope);
        let rebuilt = match rebuild::rebuild_real(s, c, job.key, history, &self.scratch) {
            Ok(v) => v,
            Err(e) if e.is::<rebuild::Availability>() || e.is::<rebuild::NodeUnavailable>() => {
                self.real_retries
                    .entry(retry_key.clone())
                    .or_default()
                    .d_node(now);
                self.alerts.push("real L1 availability; no verdict".into());
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        let worker = self
            .real_workers
            .get(&key.gate)
            .context("real gate release")?;
        if self
            .real_retries
            .get(&retry_key)
            .is_some_and(|r| r.last_canary_failed)
        {
            let pass = worker.canary(&self.envelope, &self.scratch);
            self.real_retries
                .get_mut(&retry_key)
                .expect("retry")
                .canary(now, pass);
            self.alerts
                .push("real canary recheck; no submitted attempt counted".into());
            return Ok(None);
        }
        #[cfg(test)]
        let preparation = match PREPARATION_FAULT.with(|v| v.borrow_mut().take()) {
            Some(fault) => Err(fault.into()),
            None => worker.prepare_rebuilt(rebuilt, forced, &self.scratch, deadline),
        };
        #[cfg(not(test))]
        let preparation = worker.prepare_rebuilt(rebuilt, forced, &self.scratch, deadline);
        let (input, prepared) = match preparation {
            Ok(p) => p,
            Err(e) => {
                if crate::real_deadline::is_transient(&e) {
                    self.real_retries.entry(retry_key).or_default().deferred(
                        now,
                        "T",
                        self.class_disagreement,
                    );
                    self.alerts
                        .push("DEFER(T): real preparation deadline/controller unavailable".into());
                    return Ok(None);
                }
                self.real_retries
                    .entry(retry_key.clone())
                    .or_default()
                    .d_node(now);
                self.alerts
                    .push("d-node: real artifact/release unavailable; attempt not counted".into());
                return Ok(None);
            }
        };
        let cached = self.cache.load_real(key, prepared.binding, epoch);
        let hit = cached.is_some();
        let raw = match cached {
            Some(raw) => raw,
            None => match worker.execute(&input, &prepared, &self.envelope) {
                Ok(raw) => raw,
                Err(e) => {
                    if crate::real_deadline::is_transient(&e) {
                        let retry = self.real_retries.entry(retry_key.clone()).or_default();
                        retry.deferred(now, "T", self.class_disagreement);
                        if retry.stage() == "slow" {
                            let pass = worker.canary(&self.envelope, &self.scratch);
                            self.real_retries
                                .get_mut(&retry_key)
                                .expect("retry")
                                .canary(now, pass);
                        }
                        self.alerts
                            .push("DEFER(T): real controller unavailable".into());
                    } else {
                        self.real_retries
                            .entry(retry_key.clone())
                            .or_default()
                            .d_node(now);
                        self.alerts
                            .push("d-node: real worker unavailable; attempt not counted".into());
                    }
                    return Ok(None);
                }
            },
        };
        let reply = match Reply::decode_with_statement(
            &raw,
            &prepared.descriptor,
            &prepared.context,
            &prepared.package.caps,
            forced,
            &prepared.package.authenticated,
        ) {
            Ok(r) => r,
            Err(_) => {
                self.real_retries
                    .entry(retry_key.clone())
                    .or_default()
                    .d_node(now);
                self.alerts
                    .push("d-node: malformed real reply/cache; attempt not counted".into());
                return Ok(None);
            }
        };
        match reply.outcome.as_str() {
            "ACCEPT" | "REJECT" | "VOID" => {
                let record = checked_real_record(
                    &input,
                    &prepared.descriptor,
                    job,
                    epoch,
                    &raw,
                    &prepared.package.caps,
                    forced,
                )?;
                if !hit
                    && self
                        .cache
                        .save_real(key, prepared.binding, epoch, &raw)
                        .is_err()
                {
                    self.alerts
                        .push("real cache unavailable; computed result retained".into());
                }
                self.real_retries.remove(&retry_key);
                self.evidence.push(serde_json::json!({"queue_index":job.queue_index,"epoch":epoch,"cache_hit":hit,"classification":"computed","descriptor":"boole-m3-real/v1","row":reply.row,"input_binding":boole_derivation::attributes::hex_string(&prepared.binding)}));
                Ok(Some(record))
            }
            "DEFER" => {
                if reply.failure_class != "D-node" {
                    let retry = self.real_retries.entry(retry_key.clone()).or_default();
                    retry.deferred(now, &reply.failure_class, self.class_disagreement);
                    if retry.stage() == "slow" {
                        let pass = worker.canary(&self.envelope, &self.scratch);
                        self.real_retries
                            .get_mut(&retry_key)
                            .expect("retry")
                            .canary(now, pass);
                    }
                } else {
                    self.real_retries.entry(retry_key).or_default().d_node(now);
                }
                self.alerts
                    .push(format!("real DEFER({})", reply.failure_class));
                Ok(None)
            }
            "STALL" => {
                self.real_retries.entry(retry_key).or_default().d_node(now);
                self.alerts
                    .push("real input availability; no verdict".into());
                Ok(None)
            }
            "STOP" => defer_worker_stop(
                self.real_retries.entry(retry_key).or_default(),
                &mut self.alerts,
                now,
            ),
            _ => unreachable!("closed real reply"),
        }
    }
}

pub fn checked_real_record(
    input: &GateInput,
    d: &crate::real_descriptor::Descriptor,
    job: &Job,
    epoch: u32,
    raw: &[u8],
    caps: &crate::real_reply::Caps,
    forced: bool,
) -> Result<VerdictRecord> {
    let context = crate::real_reply::Context::from_input(input);
    let package = crate::real_package::Package::decode(&input.package, d, false)?;
    let reply =
        Reply::decode_with_statement(raw, d, &context, caps, forced, &package.authenticated)?;
    let outcome = match reply.outcome.as_str() {
        "ACCEPT" => Outcome::Accept,
        "REJECT" => Outcome::Reject,
        "VOID" => Outcome::Void,
        _ => anyhow::bail!("real reply has no verdict"),
    };
    ensure!(
        job.key == (input.problem_id, input.commit_hash),
        "real adapter identity"
    );
    let statement = VerdictStatement {
        chain_id: input.chain_id,
        problem_id: input.problem_id,
        commit_hash: input.commit_hash,
        solver: input.solver,
        env_id: input.env_id,
        export_digest: input.export_digest,
        statement_id: if reply.statement_id == "0".repeat(64) {
            [0; 32]
        } else {
            hash(&reply.statement_id)?
        },
        gate_rule_digest: d.digest(),
        kernel_set_digest: hash(&d.kernel_set_digest)?,
        outcome,
        reason: reply.reason,
        ext: reply.ext,
    };
    let record = VerdictRecord {
        kind: job.kind,
        commit_hash: input.commit_hash,
        outcome,
        reason: reply.reason,
        gate_epoch: epoch,
        fuel: reply.fuel.context("terminal real fuel")?,
        vstmt_digest: statement.digest(input.lane)?,
    };
    record.encode()?;
    Ok(record)
}
impl Supervisor {
    /// Explicit local qualification seam. This does not install a Config, start
    /// a node, activate a gate, or skip any real worker/reply validation.
    #[allow(clippy::too_many_arguments)]
    pub fn qualify_real_job(
        worker: &crate::real_worker::Worker,
        input: &GateInput,
        components: &[Hash],
        job: &Job,
        epoch: u32,
        envelope: &Envelope,
        root: &Path,
    ) -> Result<(Vec<u8>, Option<VerdictRecord>)> {
        let prepared = worker.prepare_until(
            input,
            components,
            false,
            root,
            crate::real_deadline::Deadline::for_envelope(envelope),
        )?;
        let raw = worker.execute(input, &prepared, envelope)?;
        let reply = Reply::decode_with_statement(
            &raw,
            &prepared.descriptor,
            &prepared.context,
            &prepared.package.caps,
            false,
            &prepared.package.authenticated,
        )?;
        let record = if ["ACCEPT", "REJECT", "VOID"].contains(&reply.outcome.as_str()) {
            Some(checked_real_record(
                input,
                &prepared.descriptor,
                job,
                epoch,
                &raw,
                &prepared.package.caps,
                false,
            )?)
        } else {
            None
        };
        Ok((raw, record))
    }
}

impl Supervisor {
    #[allow(clippy::too_many_arguments)]
    pub fn qualify_real_files(
        worker: &crate::real_worker::Worker,
        context: &crate::real_reply::Context,
        package: Vec<u8>,
        components: &[Hash],
        job: &Job,
        epoch: u32,
        envelope: &Envelope,
        root: &Path,
    ) -> Result<(Vec<u8>, Option<VerdictRecord>)> {
        let (input, prepared) = worker.prepare_files_until(
            context,
            package,
            components,
            root,
            crate::real_deadline::Deadline::for_envelope(envelope),
        )?;
        let raw = worker.execute(&input, &prepared, envelope)?;
        let reply = Reply::decode_with_statement(
            &raw,
            &prepared.descriptor,
            &prepared.context,
            &prepared.package.caps,
            false,
            &prepared.package.authenticated,
        )?;
        let record = if ["ACCEPT", "REJECT", "VOID"].contains(&reply.outcome.as_str()) {
            Some(checked_real_record(
                &input,
                &prepared.descriptor,
                job,
                epoch,
                &raw,
                &prepared.package.caps,
                false,
            )?)
        } else {
            None
        };
        Ok((raw, record))
    }
}

/// The adapter STOP channel is not proof of corrupted anchored L1 input.
pub fn defer_worker_stop(
    retry: &mut crate::real_retry::Retry,
    alerts: &mut Vec<String>,
    now: u64,
) -> Result<Option<VerdictRecord>> {
    retry.d_node(now);
    alerts.push(
        "d-node: adapter content-address STOP; anchored L1 integrity is checked independently; no verdict".into(),
    );
    Ok(None)
}

impl Supervisor {
    /// Local compressed-transport qualification, using the actual real controller.
    #[allow(clippy::too_many_arguments)]
    pub fn qualify_real_transport(
        worker: &crate::real_worker::Worker,
        context: &crate::real_reply::Context,
        package: Vec<u8>,
        transport: &[u8],
        length: u64,
        components: &[Hash],
        job: &Job,
        epoch: u32,
        envelope: &Envelope,
        root: &Path,
    ) -> Result<(Vec<u8>, Option<VerdictRecord>)> {
        let input = context.carried_input(package)?;
        let prepared = worker.prepare_transport(
            &input,
            transport,
            length,
            components,
            root,
            crate::real_deadline::Deadline::for_envelope(envelope),
        )?;
        let raw = worker.execute(&input, &prepared, envelope)?;
        let reply = Reply::decode_with_statement(
            &raw,
            &prepared.descriptor,
            &prepared.context,
            &prepared.package.caps,
            false,
            &prepared.package.authenticated,
        )?;
        let record = if ["ACCEPT", "REJECT", "VOID"].contains(&reply.outcome.as_str()) {
            Some(checked_real_record(
                &input,
                &prepared.descriptor,
                job,
                epoch,
                &raw,
                &prepared.package.caps,
                false,
            )?)
        } else {
            None
        };
        Ok((raw, record))
    }
}

#[cfg(test)]
thread_local! { static PREPARATION_FAULT: std::cell::RefCell<Option<crate::real_controller::Fault>> = const {std::cell::RefCell::new(None)}; }
#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod support;
#[cfg(test)]
mod cause_dispatch_tests {
    use super::*;
    use crate::{
        codec::{Codec, Scratch},
        process::Envelope,
        real_controller::{Fault, Phase, PhysicalStop, Unavailable},
    };
    #[test]
    fn physical_and_node_causes_flow_through_real_collection_without_a_verdict() -> Result<()> {
        for phase in [
            Phase::Preflight,
            Phase::Decode,
            Phase::Front,
            Phase::Kernel,
            Phase::Recheck,
        ] {
            for (fault, attempts) in [
                (Fault::physical(phase, PhysicalStop::Wall), 1),
                (Fault::physical(phase, PhysicalStop::Memory), 1),
                (Fault::physical(phase, PhysicalStop::Tasks), 1),
                (Fault::physical(phase, PhysicalStop::Capture), 1),
                (Fault::unavailable(phase, Unavailable::Provisioning), 0),
            ] {
                let (c, state, _, mut history) =
                    support::fixture_with(support::scenario::m3_cfg(), Some(0));
                let forks = ForkTable::genesis(&c)?;
                let gate = forks.rows[0].gate_rule_digest;
                let worker: crate::real_worker::Worker = serde_json::from_value(
                    serde_json::json!({"descriptor":"must-not-read","gate_rule_digest":gate,"python":"must-not-run","python_sha256":vec![2;32],"scripts":"must-not-read","tools_root":"must-not-read","counter_driver":"must-not-run","resolver":{"root":"missing","manifests":{}},"public_counter":false,"canary":null}),
                )?;
                let root = Scratch::new(&std::env::temp_dir())?;
                let mut supervisor = Supervisor::new_releases(
                    &c,
                    &forks,
                    Mode::Follow {
                        from: 1,
                        through: 100,
                    },
                    vec![Release::Real(Box::new(worker))],
                    Some(Codec::portable_stub()),
                    &root.0.join("cache"),
                    &root.0,
                    &root.0,
                    Host {
                        profile: "darwin-aarch64".into(),
                        ram_mib: 32768,
                        cpus: 4,
                    },
                    Envelope::default(),
                )?;
                // Only this unit fixture seeds the already-validated installation.
                // Production startup still has to validate the complete closure.
                supervisor.mode = Mode::Verify;
                supervisor.verified_table = Some((c.digest(), forks.rows.clone()));
                PREPARATION_FAULT.with(|v| *v.borrow_mut() = Some(fault));
                let result =
                    supervisor.collect_at(&state, &c, state.height() + 1, &forks, &mut history, 10);
                let unconsumed = PREPARATION_FAULT.with(|v| v.borrow_mut().take());
                assert!(
                    unconsumed.is_none(),
                    "real preparation path did not consume the fault"
                );
                assert!(result?.is_empty());
                assert_eq!(supervisor.real_retries.len(), 1);
                let retry = supervisor.real_retries.values().next().unwrap();
                assert_eq!(retry.attempts, attempts);
                assert_eq!(retry.next_at, 70);
                assert!(supervisor.alerts.iter().any(|a| if attempts == 1 {
                    a.starts_with("DEFER(T)")
                } else {
                    a.starts_with("d-node:")
                }));
            }
        }
        Ok(())
    }
}
