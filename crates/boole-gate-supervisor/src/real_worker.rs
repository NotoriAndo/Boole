//! Descriptor-selected Mac development worker over the data-only Python entry.
use crate::{
    codec::{digest, private_file, read_bounded, Scratch},
    process::Envelope,
    real_artifact::{Artifact, Resolver},
    real_descriptor::{ascii, hash, Descriptor},
    real_package::Package,
    real_process,
    real_reply::{Context as JobContext, Reply},
    GateInput,
};
use anyhow::{ensure, Context, Result};
use boole_derivation::m2::Hash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Worker {
    pub descriptor: PathBuf,
    pub gate_rule_digest: Hash,
    pub python: PathBuf,
    pub python_sha256: Hash,
    pub scripts: PathBuf,
    pub tools_root: PathBuf,
    pub counter_driver: PathBuf,
    pub resolver: Resolver,
    pub public_counter: bool,
    pub canary: Option<Canary>,
    #[serde(default)]
    pub decoder: Option<crate::real_decoder::Config>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Canary {
    pub context: JobContext,
    pub expected_reply_sha256: Hash,
}
pub struct Prepared {
    pub descriptor: Descriptor,
    pub context: JobContext,
    pub package: Package,
    pub binding: Hash,
    pub scratch: Scratch,
    artifacts: Vec<Artifact>,
    inventory: PathBuf,
    reference: PathBuf,
    certified: Option<PathBuf>,
    bundle_file: Option<PathBuf>,
    pub fork_authorized: bool,
    pub local: Option<Vec<u8>>,
    decoder_evidence: Option<crate::real_decoder::Decoded>,
    deadline: crate::real_deadline::Deadline,
}
pub fn file_hash(path: &Path) -> Result<Hash> {
    file_hash_until(path, crate::real_deadline::Deadline::after(30000))
}
pub fn file_hash_until(path: &Path, deadline: crate::real_deadline::Deadline) -> Result<Hash> {
    deadline.check()?;
    let path = fs::canonicalize(path)?;
    let before = fs::metadata(&path)?;
    ensure!(before.is_file(), "pinned file type");
    let mut file = File::open(&path)?;
    let mut h = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = deadline.read(&mut file, &mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let after = file.metadata()?;
    ensure!(
        before.len() == after.len() && before.modified()? == after.modified()?,
        "pinned file changed"
    );
    Ok(h.finalize().into())
}
fn source_hash(scripts: &Path, deadline: crate::real_deadline::Deadline) -> Result<Hash> {
    let root = fs::canonicalize(scripts.join("zk_registry"))?;
    let mut files = vec![];
    let mut pending = vec![root.clone()];
    while let Some(dir) = pending.pop() {
        deadline.check()?;
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            ensure!(!ty.is_symlink(), "Python release symlink");
            if ty.is_dir() {
                pending.push(entry.path());
            } else if entry.path().extension().is_some_and(|v| v == "py") {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    let mut h = Sha256::new();
    h.update(b"BOOLE-M3/REAL-GATE-SOURCE/v1\0");
    for path in files {
        let name = path
            .strip_prefix(&root)?
            .to_str()
            .context("source UTF-8")?
            .replace('\\', "/");
        let bytes = deadline.read_bounded(&path, 256 * 1024 * 1024)?;
        h.update((name.len() as u32).to_be_bytes());
        h.update(name.as_bytes());
        h.update((bytes.len() as u64).to_be_bytes());
        for chunk in bytes.chunks(65536) {
            deadline.check()?;
            h.update(chunk);
        }
    }
    deadline.check()?;
    Ok(h.finalize().into())
}
impl Worker {
    pub fn descriptor(&self) -> Result<Descriptor> {
        let raw = read_bounded(&self.descriptor, 65536)?;
        let d = Descriptor::decode(&raw)?;
        ensure!(
            d.digest() == self.gate_rule_digest,
            "S16: real descriptor identity"
        );
        Ok(d)
    }
    fn command(&self, root: &Path) -> Result<Command> {
        let mut c = Command::new(fs::canonicalize(&self.python)?);
        c.env_clear()
            .env("PYTHONPATH", fs::canonicalize(&self.scripts)?)
            .env("PYTHONNOUSERSITE", "1")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("PYTHONHASHSEED", "0")
            .env("PYTHONPYCACHEPREFIX", root.join("pycache"))
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C");
        Ok(c)
    }
    pub fn validate(&self) -> Result<Descriptor> {
        self.validate_until(crate::real_deadline::Deadline::after(30000))
    }
    fn validate_until(&self, deadline: crate::real_deadline::Deadline) -> Result<Descriptor> {
        deadline.check()?;
        crate::real_linux::select(&crate::process::host_profile())?;
        let d = self.descriptor()?;
        ensure!(
            file_hash_until(&self.python, deadline)? == self.python_sha256,
            "S16: Python binary pin"
        );
        ensure!(
            source_hash(&self.scripts, deadline)? == hash(&d.source_sha256)?,
            "S16: Python source release"
        );
        for (name, pin) in [
            (
                "acceptance_environment_v2.json",
                &d.acceptance_environment_sha256,
            ),
            ("acceptance_tools_v2.json", &d.acceptance_tools_sha256),
            ("fuel_cap_authority_v1.json", &d.fuel_authority_sha256),
            ("testnet_caps_v2.json", &d.caps_sha256),
        ] {
            ensure!(
                file_hash_until(&self.scripts.join("zk_registry/data").join(name), deadline)?
                    == hash(pin)?,
                "S16: real release data pin"
            );
        }
        let data: serde_json::Value = serde_json::from_slice(
            &deadline.read_bounded(
                &self
                    .scripts
                    .join("zk_registry/data/acceptance_tools_v2.json"),
                1048576,
            )?,
        )?;
        let pins = &data["binary_profiles"]["Darwin-arm64"];
        let tools = serde_json::to_value(&d.profiles["Darwin-arm64"].tools)?;
        for name in ["comparator", "nanoda", "exporter", "lean_replay"] {
            let relative = pins[name]["path"].as_str().context("tool path binding")?;
            ensure!(
                file_hash_until(&self.tools_root.join(relative), deadline)?
                    == hash(tools[name].as_str().context("tool pin")?)?,
                "S16: native tool pin"
            );
        }
        ensure!(
            file_hash_until(&self.counter_driver, deadline)?
                == hash(&d.profiles["Darwin-arm64"].tools.counter_replay)?,
            "S16: counter driver pin"
        );
        let probe = Scratch::new(&std::env::temp_dir())?;
        let output=real_process::run(self.command(&probe.0)?.args(["-B","-c","import sys;print(sys.implementation.name+':'+'.'.join(map(str,sys.version_info[:3])))"]),&Envelope{memory_mib:512,wall_ms:deadline.remaining()?.min(5000),output_cap:128})?;
        ensure!(
            output.status.success() && output.stdout == b"cpython:3.9.6\n",
            "S16: reference Python interpreter"
        );
        Ok(d)
    }
    pub fn prepare(
        &self,
        input: &GateInput,
        components: &[Hash],
        fork_authorized: bool,
        root: &Path,
    ) -> Result<Prepared> {
        self.prepare_until(
            input,
            components,
            fork_authorized,
            root,
            crate::real_deadline::Deadline::after(Envelope::default().wall_ms),
        )
    }
    pub fn prepare_until(
        &self,
        input: &GateInput,
        components: &[Hash],
        fork_authorized: bool,
        root: &Path,
        deadline: crate::real_deadline::Deadline,
    ) -> Result<Prepared> {
        self.prepare_inner(
            input,
            components,
            fork_authorized,
            root,
            false,
            deadline,
            None,
        )
    }
    #[allow(clippy::too_many_arguments)] // Keep input and trusted controller bindings explicit.
    fn prepare_inner(
        &self,
        input: &GateInput,
        components: &[Hash],
        fork_authorized: bool,
        root: &Path,
        file_bundle: bool,
        deadline: crate::real_deadline::Deadline,
        encoded: Option<(&[u8], u64)>,
    ) -> Result<Prepared> {
        let d = self.validate_until(deadline)?;
        let scratch = Scratch::new(root)?;
        let context = JobContext::from_input(input);
        context.validate()?;
        ensure!(
            input.raw.len() as u64 <= boole_derivation::m2::RAW_CAP && input.artifacts.is_empty(),
            "real worker read/stdin limits"
        );
        let binding = input_binding(input, components, fork_authorized, self.public_counter)?;
        let inventory =
            self.resolver
                .resolve_until(hash(&d.artifact_table_sha256)?, &scratch.0, deadline)?;
        let inventory_path = inventory.path.clone();
        let inventory_bytes = deadline.read_bounded(&inventory_path, 256 * 1024 * 1024)?;
        let (package, mut local) = crate::real_admission::authenticate(
            &input.package,
            &inventory_bytes,
            &d,
            &context,
            self.public_counter,
            fork_authorized,
        )?;
        let mut artifacts = vec![inventory];
        if local.is_none() && !file_bundle && encoded.is_none() {
            let row = match input.early {
                Some((2, 0x0101)) => Some("front_invalid"),
                Some((2, 0x0102)) => Some("input_integrity"),
                Some((3, 0x0801)) => Some("package_invalid"),
                Some(_) => anyhow::bail!("unauthorized reconstruction result"),
                None => None,
            };
            if let Some(row) = row {
                local = Some(Reply::reconstructed_with_statement(
                    row,
                    &d,
                    &context,
                    &package.caps,
                    false,
                    &package.authenticated,
                )?);
            } else {
                local = crate::real_admission::transport(
                    &d,
                    &context,
                    &package,
                    Some(&input.raw),
                    None,
                )?;
            }
        }
        let mut decoder_evidence = None;
        if local.is_none() {
            if let Some((transport, length)) = encoded {
                let config = self
                    .decoder
                    .as_ref()
                    .context("typed real decoder not provisioned")?;
                let mut decoded = config.decode(
                    transport,
                    length,
                    &d,
                    &context,
                    &self.python,
                    &scratch.0,
                    deadline,
                )?;
                local = crate::real_admission::transport(
                    &d,
                    &context,
                    &package,
                    decoded.raw.as_deref(),
                    Some((&ascii(&decoded.status)?, decoded.pin, decoded.transport)),
                )?;
                decoded.raw = None;
                decoder_evidence = Some(decoded);
            }
        }
        if local.is_some() {
            deadline.check()?;
            return Ok(Prepared {
                descriptor: d,
                context,
                package,
                binding,
                scratch,
                artifacts,
                inventory: inventory_path,
                reference: PathBuf::new(),
                certified: None,
                bundle_file: None,
                fork_authorized,
                local,
                deadline,
                decoder_evidence,
            });
        }
        let bundle_file = if let Some(decoded) = &decoder_evidence {
            decoded.path.clone()
        } else if file_bundle {
            ensure!(
                input.early.is_none(),
                "file qualification cannot authorize early/fork results"
            );
            let blob = self
                .resolver
                .resolve_until(input.export_digest, &scratch.0, deadline)?;
            ensure!(
                blob.bytes <= 256 * 1024 * 1024,
                "file qualification read cap"
            );
            let path = blob.path.clone();
            artifacts.push(blob);
            Some(path)
        } else {
            None
        };
        let bundle_path = bundle_file
            .clone()
            .unwrap_or_else(|| scratch.0.join("bundle.bin"));
        if bundle_file.is_none() {
            deadline.write_file(&bundle_path, &input.raw)?;
        }
        for (name, raw) in [
            ("descriptor.json", ascii(&d)?),
            ("context.json", ascii(&context)?),
            ("package.json", input.package.clone()),
        ] {
            deadline.write_file(&scratch.0.join(name), &raw)?;
        }
        if let Some(decoded) = &decoder_evidence {
            deadline.write_file(
                &scratch.0.join("decoder-status.json"),
                &ascii(&decoded.status)?,
            )?;
        }
        let first = self.front_phase(
            &d,
            &context,
            &scratch.0,
            &inventory_path,
            &bundle_path,
            &scratch.0.join("not-materialized-reference"),
            decoder_evidence.as_ref(),
            "reference",
            deadline,
        )?;
        if let Some(raw) = first.reply()? {
            crate::real_front::check_reply(
                &raw,
                &d,
                &context,
                &package.caps,
                &package.authenticated,
            )?;
            return Ok(Prepared {
                descriptor: d,
                context,
                package,
                binding,
                scratch,
                artifacts,
                inventory: inventory_path,
                reference: PathBuf::new(),
                certified: None,
                bundle_file: Some(bundle_path),
                fork_authorized,
                local: Some(raw),
                deadline,
                decoder_evidence,
            });
        }
        ensure!(
            first.need == "reference",
            "front must request reference before materialization"
        );
        let reference = if let Some(id) = package.reference {
            if let Some(a) = artifacts.iter().find(|a| a.sha256 == id) {
                a.path.clone()
            } else {
                let a = self.resolver.resolve_until(id, &scratch.0, deadline)?;
                let p = a.path.clone();
                artifacts.push(a);
                p
            }
        } else {
            let p = scratch.0.join("no-reference");
            private_file(&p, &[])?;
            p
        };
        let second = self.front_phase(
            &d,
            &context,
            &scratch.0,
            &inventory_path,
            &bundle_path,
            &reference,
            decoder_evidence.as_ref(),
            "state",
            deadline,
        )?;
        if let Some(raw) = second.reply()? {
            crate::real_front::check_reply(
                &raw,
                &d,
                &context,
                &package.caps,
                &package.authenticated,
            )?;
            return Ok(Prepared {
                descriptor: d,
                context,
                package,
                binding,
                scratch,
                artifacts,
                inventory: inventory_path,
                reference,
                certified: None,
                bundle_file: Some(bundle_path),
                fork_authorized,
                local: Some(raw),
                deadline,
                decoder_evidence,
            });
        }
        ensure!(
            (package.certified.is_some() && second.need == "state")
                || (package.certified.is_none() && second.need == "ready"),
            "unexpected state materialization request"
        );
        // Deduplicate full content addresses before publishing into private scratch.
        for id in components {
            if !artifacts.iter().any(|a| a.sha256 == *id) {
                artifacts.push(self.resolver.resolve_until(*id, &scratch.0, deadline)?);
            }
        }
        let certified = if let Some((environment, id)) = &package.certified {
            let path = if let Some(a) = artifacts.iter().find(|a| a.sha256 == *id) {
                a.path.clone()
            } else {
                let a = self.resolver.resolve_until(*id, &scratch.0, deadline)?;
                let p = a.path.clone();
                artifacts.push(a);
                p
            };
            let provision = scratch.0.join("certified");
            let directory = provision.join(environment);
            fs::create_dir_all(&directory)?;
            fs::hard_link(path, directory.join("state.olean"))?;
            Some(provision)
        } else {
            None
        };
        Ok(Prepared {
            descriptor: d,
            context,
            package,
            binding,
            scratch,
            artifacts,
            inventory: inventory_path,
            reference,
            certified,
            bundle_file: Some(bundle_path),
            fork_authorized,
            local,
            deadline,
            decoder_evidence,
        })
    }
    #[allow(clippy::too_many_arguments)]
    fn front_phase(
        &self,
        d: &Descriptor,
        c: &JobContext,
        root: &Path,
        inventory: &Path,
        bundle: &Path,
        reference: &Path,
        decoded: Option<&crate::real_decoder::Decoded>,
        mode: &str,
        deadline: crate::real_deadline::Deadline,
    ) -> Result<crate::real_front::Phase> {
        let request = serde_json::json!({"descriptor":root.join("descriptor.json"),"gate":boole_derivation::attributes::hex_string(&self.gate_rule_digest)[2..],"context":root.join("context.json"),"package":root.join("package.json"),"inventory":inventory,"bundle":bundle,"reference":reference,"tools_root":fs::canonicalize(&self.tools_root)?,"counter_driver":fs::canonicalize(&self.counter_driver)?,"work":root.join(format!("front-{mode}")),"mode":mode,"public_counter":self.public_counter,"decoder_status":decoded.map(|_|root.join("decoder-status.json")),"decoder_pin":decoded.map(|v|boole_derivation::attributes::hex_string(&v.pin)[2..].to_owned()),"transport_digest":decoded.map(|v|boole_derivation::attributes::hex_string(&v.transport)[2..].to_owned())});
        let path = root.join(format!("front-request-{mode}.json"));
        deadline.write_file(&path, &serde_json::to_vec(&request)?)?;
        let output = real_process::run(
            self.command(root)?
                .args(["-B", "-c", include_str!("real_front.py")])
                .arg(path)
                .current_dir(root)
                .env("TMPDIR", root),
            &Envelope {
                memory_mib: deadline.memory_mib(),
                wall_ms: deadline.remaining()?,
                output_cap: 65536,
            },
        )?;
        ensure!(
            output.status.success(),
            "private front preparation unavailable"
        );
        crate::real_front::Phase::decode(&output.stdout, d, c, mode, self.public_counter, deadline)
    }
    pub fn execute(
        &self,
        _input: &GateInput,
        p: &Prepared,
        envelope: &Envelope,
    ) -> Result<Vec<u8>> {
        p.deadline.check()?;
        if let Some(raw) = &p.local {
            p.recheck_after_run(envelope)?;
            return Ok(raw.clone());
        }
        let root = &p.scratch.0;
        let descriptor = root.join("descriptor.json");
        let context = root.join("context.json");
        let package = root.join("package.json");
        let bundle = p
            .bundle_file
            .clone()
            .unwrap_or_else(|| root.join("bundle.bin"));
        let mut command = self.command(root)?;
        command
            .args(["-B", "-m", "zk_registry.m3_gate", "--descriptor"])
            .arg(descriptor)
            .arg("--gate-digest")
            .arg(&boole_derivation::attributes::hex_string(&self.gate_rule_digest)[2..])
            .arg("--bundle")
            .arg(bundle)
            .arg("--package")
            .arg(package)
            .arg("--inventory")
            .arg(&p.inventory)
            .arg("--context")
            .arg(context)
            .arg("--reference")
            .arg(&p.reference)
            .arg("--tools-root")
            .arg(fs::canonicalize(&self.tools_root)?)
            .arg("--counter-driver")
            .arg(fs::canonicalize(&self.counter_driver)?)
            .arg("--work")
            .arg(root.join("adapter"))
            .current_dir(root)
            .env("TMPDIR", root);
        if let Some(decoded) = &p.decoder_evidence {
            let status = root.join("decoder-status.json");
            command
                .arg("--decoder-status")
                .arg(status)
                .arg("--decoder-pin")
                .arg(&boole_derivation::attributes::hex_string(&decoded.pin)[2..])
                .arg("--transport-digest")
                .arg(&boole_derivation::attributes::hex_string(&decoded.transport)[2..]);
        }
        if let Some(certified) = &p.certified {
            command.arg("--certified-root").arg(certified);
        }
        if self.public_counter {
            command.arg("--public-counter");
        }
        let output = real_process::run(
            &mut command,
            &Envelope {
                wall_ms: p.deadline.remaining()?.min(envelope.wall_ms),
                ..envelope.clone()
            },
        )?;
        ensure!(output.status.success(), "untyped real worker exit");
        let line = output
            .stdout
            .strip_suffix(b"\n")
            .context("missing real final newline")?;
        ensure!(!line.contains(&b'\n'), "multiple real final records");
        let raw = line
            .strip_prefix(b"BOOLE-M3/GATE/v1 ")
            .context("missing real final marker")?
            .to_vec();
        Reply::decode_with_statement(
            &raw,
            &p.descriptor,
            &p.context,
            &p.package.caps,
            p.fork_authorized,
            &p.package.authenticated,
        )?;
        p.recheck_after_run(envelope)?;
        Ok(raw)
    }
    /// Local file qualification retains the adapter's oversized-bundle verdict
    /// without changing M2's 128 MiB admission or copying artifacts into stdin.
    pub fn prepare_files(
        &self,
        context: &JobContext,
        package: Vec<u8>,
        components: &[Hash],
        root: &Path,
    ) -> Result<(GateInput, Prepared)> {
        self.prepare_files_until(
            context,
            package,
            components,
            root,
            crate::real_deadline::Deadline::after(Envelope::default().wall_ms),
        )
    }
    pub fn prepare_files_until(
        &self,
        context: &JobContext,
        package: Vec<u8>,
        components: &[Hash],
        root: &Path,
        deadline: crate::real_deadline::Deadline,
    ) -> Result<(GateInput, Prepared)> {
        context.validate()?;
        ensure!(
            digest(&package) == hash(&context.package_sha256)?,
            "file package binding"
        );
        let mut solver = [0; 20];
        for (i, b) in solver.iter_mut().enumerate() {
            *b = u8::from_str_radix(&context.solver[i * 2..i * 2 + 2], 16)?;
        }
        let input = GateInput {
            chain_id: context.chain_id,
            problem_id: hash(&context.problem_id)?,
            commit_hash: hash(&context.commit_hash)?,
            solver,
            env_id: hash(&context.env_id)?,
            export_digest: hash(&context.export_digest)?,
            lane: context.lane,
            leaf_kind: context.leaf_kind,
            instance_seed: [0; 32],
            fuel_slot: context.slot_fuel,
            raw: vec![],
            package,
            artifacts: vec![],
            early: None,
        };
        let prepared = self.prepare_inner(&input, components, false, root, true, deadline, None)?;
        Ok((input, prepared))
    }
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_transport(
        &self,
        input: &GateInput,
        transport: &[u8],
        length: u64,
        components: &[Hash],
        root: &Path,
        deadline: crate::real_deadline::Deadline,
    ) -> Result<Prepared> {
        self.prepare_inner(
            input,
            components,
            false,
            root,
            false,
            deadline,
            Some((transport, length)),
        )
    }
    pub fn prepare_rebuilt(
        &self,
        mut rebuilt: crate::real_rebuild::Rebuilt,
        forced: bool,
        root: &Path,
        deadline: crate::real_deadline::Deadline,
    ) -> Result<(GateInput, Prepared)> {
        if rebuilt.package_encoding == 1 {
            let d = self.validate_until(deadline)?;
            let scratch = Scratch::new(root)?;
            let context = JobContext::from_input(&rebuilt.input);
            let decoder = self
                .decoder
                .as_ref()
                .context("typed package decoder unavailable")?;
            let decoded = decoder.decode_package(
                &rebuilt.package_transport,
                rebuilt.package_raw_len,
                &d,
                &context,
                &self.python,
                &scratch.0,
                deadline,
            )?;
            match decoded.status.status.as_str() {
                "ok" => {
                    rebuilt.input.package = decoded.raw.context("package decoded output absent")?
                }
                "invalid-data" => rebuilt.input.package = rebuilt.package_transport,
                _ => anyhow::bail!("typed package decoder unavailable"),
            }
        }
        let p = self.prepare_inner(
            &rebuilt.input,
            &rebuilt.components,
            forced,
            root,
            false,
            deadline,
            Some((&rebuilt.transport, rebuilt.raw_len)),
        )?;
        Ok((rebuilt.input, p))
    }
    pub fn canary(&self, envelope: &Envelope, root: &Path) -> bool {
        let Some(canary) = &self.canary else {
            return false;
        };
        (|| -> Result<bool> {
            let deadline = crate::real_deadline::Deadline::for_envelope(envelope);
            let staging = Scratch::new(root)?;
            let bundle = self.resolver.resolve_until(
                hash(&canary.context.export_digest)?,
                &staging.0,
                deadline,
            )?;
            let package = self.resolver.resolve_until(
                hash(&canary.context.package_sha256)?,
                &staging.0,
                deadline,
            )?;
            let input = canary.context.input(
                deadline.read_bounded(&bundle.path, boole_derivation::m2::RAW_CAP as usize)?,
                deadline.read_bounded(&package.path, 1048576)?,
            )?;
            let prepared = self.prepare_until(&input, &[], false, root, deadline)?;
            let raw = self.execute(&input, &prepared, envelope)?;
            Ok(digest(&raw) == canary.expected_reply_sha256
                && serde_json::from_slice::<Reply>(&raw)?.outcome == "ACCEPT")
        })()
        .unwrap_or(false)
    }
}

pub fn input_binding(
    input: &GateInput,
    components: &[Hash],
    fork_authorized: bool,
    public_counter: bool,
) -> Result<Hash> {
    Ok(digest(&ascii(&(
        "BOOLE-M3/REAL-INPUT/v2",
        JobContext::from_input(input).digest()?,
        input.instance_seed,
        input.early,
        components,
        fork_authorized,
        public_counter,
    ))?))
}
impl Worker {
    pub fn validate_config(&self, c: &boole_derivation::m2::Config, verify: bool) -> Result<()> {
        ensure!(
            c.statement_raw_max <= 1048576,
            "S16: Config statement package exceeds worker admission"
        );
        // The closed v1 descriptor has no public-counter mode assignment.
        ensure!(
            !verify || !self.public_counter,
            "S16: public-counter qualification is not named by this verify descriptor"
        );
        Ok(())
    }
}

#[cfg(test)]
mod pycache_tests {
    use super::*;
    #[test]
    fn stale_unpinned_pyc_is_not_read_from_the_source_tree() -> Result<()> {
        let root = Scratch::new(&std::env::temp_dir())?;
        let python = PathBuf::from("/usr/bin/python3");
        let w = Worker {
            descriptor: root.0.clone(),
            gate_rule_digest: [1; 32],
            python: python.clone(),
            python_sha256: [2; 32],
            scripts: root.0.clone(),
            tools_root: root.0.clone(),
            counter_driver: root.0.clone(),
            resolver: Resolver {
                root: root.0.clone(),
                manifests: Default::default(),
            },
            public_counter: false,
            canary: None,
            decoder: None,
        };
        let script="import pathlib,py_compile,os,sys; pathlib.Path('__pycache__').mkdir(); p=pathlib.Path('probe.py'); p.write_text('VALUE = \"EVIL\"'); py_compile.compile(str(p),cfile='__pycache__/probe.'+sys.implementation.cache_tag+'.pyc',invalidation_mode=py_compile.PycInvalidationMode.TIMESTAMP); s=p.stat(); p.write_text('VALUE = \"SAFE\"'); os.utime(p,ns=(s.st_atime_ns,s.st_mtime_ns))";
        ensure!(
            Command::new(python)
                .args(["-B", "-c", script])
                .current_dir(&root.0)
                .status()?
                .success(),
            "fixture pyc creation"
        );
        let unsafe_control = Command::new(&w.python)
            .env_clear()
            .args(["-B", "-c", "import probe;print(probe.VALUE)"])
            .current_dir(&root.0)
            .output()?;
        assert_eq!(
            unsafe_control.stdout, b"EVIL\n",
            "fixture must exercise an existing valid source-tree pyc"
        );
        let out = w
            .command(&root.0)?
            .args(["-B", "-c", "import probe;print(probe.VALUE)"])
            .current_dir(&root.0)
            .output()?;
        assert!(out.status.success());
        assert_eq!(out.stdout, b"SAFE\n");
        Ok(())
    }
}

impl Prepared {
    fn recheck_after_run(&self, envelope: &Envelope) -> Result<()> {
        let deadline = crate::real_deadline::Deadline::for_envelope(&Envelope {
            wall_ms: envelope.wall_ms.min(30_000),
            ..envelope.clone()
        });
        for artifact in &self.artifacts {
            artifact.verify_until(deadline)?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod deadline_tests {
    use super::*;
    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn preparation_keeps_the_absolute_deadline_through_a_slow_pinned_file_read() -> Result<()> {
        let root = Scratch::new(&std::env::temp_dir())?;
        let descriptor = root.0.join("descriptor");
        std::fs::write(
            &descriptor,
            include_bytes!("../../../fixtures/m3-real-gate/descriptor-v1.json"),
        )?;
        let d = Descriptor::decode(&std::fs::read(&descriptor)?)?;
        let python = root.0.join("python");
        std::fs::write(&python, b"pinned interpreter fixture")?;
        let worker = Worker {
            descriptor,
            gate_rule_digest: d.digest(),
            python_sha256: digest(b"pinned interpreter fixture"),
            python,
            scripts: root.0.join("unreached-sdk"),
            tools_root: root.0.clone(),
            counter_driver: root.0.clone(),
            resolver: Resolver {
                root: root.0.clone(),
                manifests: Default::default(),
            },
            public_counter: false,
            canary: None,
            decoder: None,
        };
        let v: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../fixtures/m3-real-gate/reply-cases.json"
        ))?;
        let mut c: JobContext = serde_json::from_value(v["context"].clone())?;
        c.package_sha256 = hex::encode(digest(&[]));
        let input = c.carried_input(vec![])?;
        let (result, reads) = crate::real_deadline::slow_reads(0, 300, || {
            worker.prepare_until(
                &input,
                &[],
                false,
                &root.0,
                crate::real_deadline::Deadline::after(200),
            )
        });
        let error = result
            .err()
            .context("slow preparation unexpectedly completed")?;
        ensure!(
            error.is::<crate::real_deadline::Expired>()
                && crate::real_deadline::is_transient(&error),
            "{error:#}"
        );
        ensure!(reads == 1, "preparation did not reach pinned reader");
        Ok(())
    }
    #[test]
    fn post_run_recheck_does_not_consume_the_exhausted_kernel_deadline() -> Result<()> {
        let scratch = Scratch::new(&std::env::temp_dir())?;
        let path = scratch.0.join("artifact");
        std::fs::write(&path, b"data")?;
        let path = std::fs::canonicalize(path)?;
        let d = Descriptor::decode(include_bytes!(
            "../../../fixtures/m3-real-gate/descriptor-v1.json"
        ))?;
        let vectors: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../fixtures/m3-real-gate/reply-cases.json"
        ))?;
        let p = Prepared {
            package: Package::decode(b"", &d, true)?,
            descriptor: d,
            context: serde_json::from_value(vectors["context"].clone())?,
            binding: [0; 32],
            artifacts: vec![Artifact {
                path,
                sha256: digest(b"data"),
                bytes: 4,
                max_read: 65536,
            }],
            scratch,
            inventory: PathBuf::new(),
            reference: PathBuf::new(),
            certified: None,
            bundle_file: None,
            fork_authorized: false,
            local: None,
            decoder_evidence: None,
            deadline: crate::real_deadline::Deadline::after(0),
        };
        p.recheck_after_run(&Envelope::default())?;
        let error = p
            .recheck_after_run(&Envelope {
                wall_ms: 0,
                ..Envelope::default()
            })
            .unwrap_err();
        ensure!(
            error.is::<crate::real_deadline::Expired>()
                && crate::real_deadline::is_transient(&error)
        );
        Ok(())
    }
}
