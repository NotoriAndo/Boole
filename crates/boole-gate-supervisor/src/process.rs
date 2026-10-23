//! Authenticated worker snapshots, one enforced-sandbox process per invocation.
use crate::{
    codec::{digest, private_file, read_bounded, Scratch},
    stub::Descriptor,
    GateInput, GateReply,
};
use anyhow::{ensure, Context, Result};
use boole_derivation::m2::Hash;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, Permissions},
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Clone, Debug)]
pub struct Envelope {
    pub memory_mib: u64,
    pub wall_ms: u64,
    pub output_cap: usize,
}
impl Default for Envelope {
    fn default() -> Self {
        Self {
            memory_mib: 1024,
            wall_ms: 30_000,
            output_cap: 65536,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Worker {
    pub descriptor: Descriptor,
    pub binary: PathBuf,
    pub binary_sha256: Hash,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packet {
    pub descriptor: Descriptor,
    pub input: GateInput,
}
pub fn host_profile() -> String {
    format!(
        "{}-{}",
        if std::env::consts::OS == "macos" {
            "darwin"
        } else {
            std::env::consts::OS
        },
        std::env::consts::ARCH
    )
}
const MARKER: &str = "BOOLE-M3/GATE/v1 ";
impl Worker {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.descriptor.binaries.get(&host_profile()) == Some(&self.binary_sha256),
            "S16: binary pin is not bound by the gate descriptor"
        );
        ensure!(
            digest(&read_bounded(&self.binary, 128 * 1024 * 1024)?) == self.binary_sha256,
            "S16: gate binary digest mismatch"
        );
        Ok(())
    }
    pub fn canary(&self, envelope: &Envelope, root: &Path) -> bool {
        let raw = crate::stub::export(0, 1);
        let input = GateInput {
            chain_id: 720926,
            problem_id: [1; 32],
            commit_hash: [2; 32],
            solver: [3; 20],
            env_id: [4; 32],
            export_digest: digest(&raw),
            lane: 1,
            leaf_kind: 1,
            instance_seed: [5; 32],
            fuel_slot: 10000,
            raw,
            package: crate::stub::package(10000),
            artifacts: vec![],
            early: None,
        };
        self.run(&input, envelope, root) == crate::stub::evaluate(&self.descriptor, &input)
    }
    pub fn run(&self, input: &GateInput, envelope: &Envelope, root: &Path) -> GateReply {
        self.try_run(input, envelope, root)
            .unwrap_or_else(|_| GateReply::Defer {
                class: "d-node".into(),
            })
    }
    fn try_run(&self, input: &GateInput, envelope: &Envelope, root: &Path) -> Result<GateReply> {
        self.descriptor.validate(input.chain_id, &host_profile())?;
        ensure!(
            envelope.memory_mib >= 64
                && envelope.memory_mib <= 12288
                && envelope.wall_ms > 0
                && envelope.output_cap >= 128,
            "invalid node envelope"
        );
        let binary = read_bounded(&self.binary, 128 * 1024 * 1024)?;
        ensure!(
            digest(&binary) == self.binary_sha256,
            "gate executable changed"
        );
        let scratch = Scratch::new(root)?;
        let executable = scratch.0.join("gate");
        private_file(&executable, &binary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, Permissions::from_mode(0o500))?;
        }
        let path = scratch.0.join("input.json");
        private_file(
            &path,
            &serde_json::to_vec(&Packet {
                descriptor: self.descriptor.clone(),
                input: input.clone(),
            })?,
        )?;
        let mut command = Command::new(&executable);
        command.arg("worker").current_dir(&scratch.0);
        let out = boole_lean_runner::run_byte_adapter(
            command,
            File::open(&path)?,
            &scratch.0,
            envelope.memory_mib,
            envelope.wall_ms,
            envelope.output_cap,
        )?;
        ensure!(
            out.success && out.exit_code == 0 && !out.unavailable,
            "untyped worker exit or containment failure"
        );
        let line = out
            .stdout
            .lines()
            .rev()
            .find(|s| !s.is_empty())
            .context("missing typed final marker")?;
        let reply =
            serde_json::from_str(line.strip_prefix(MARKER).context("unknown final marker")?)?;
        Ok(reply)
    }
}
pub fn worker_main() -> Result<()> {
    use std::io::Read;
    ensure!(
        std::env::args().nth(1).as_deref() == Some("worker"),
        "expected worker entrypoint"
    );
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(600 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 600 * 1024 * 1024, "worker input cap");
    let packet: Packet = serde_json::from_slice(&bytes)?;
    packet
        .descriptor
        .validate(packet.input.chain_id, &host_profile())?;
    ensure!(
        packet.descriptor.binaries.get(&host_profile())
            == Some(&digest(&read_bounded(
                &std::env::current_exe()?,
                128 * 1024 * 1024
            )?)),
        "worker descriptor binary binding"
    );
    ensure!(
        packet.descriptor.implementation_sha256 == crate::implementation_digest(),
        "worker source release binding"
    );
    let prefix = b"BOOLE-M3-STUB/v1";
    if packet.input.early.is_none() && packet.input.raw.starts_with(prefix) {
        match packet.input.raw.get(prefix.len()) {
            Some(8) => std::process::exit(17),
            Some(9) => {
                println!("unknown-worker-marker");
                return Ok(());
            }
            _ => {}
        }
    }
    println!(
        "{MARKER}{}",
        serde_json::to_string(&crate::stub::evaluate(&packet.descriptor, &packet.input))?
    );
    Ok(())
}
