//! Real-only provenance channel. Exit codes and stderr never classify data.
use crate::{
    codec::digest,
    real_deadline::Deadline,
    real_descriptor::{ascii, hash, Descriptor},
    real_reply::Context,
    real_worker::file_hash_until,
};
use anyhow::{ensure, Result};
use boole_derivation::m2::Hash;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub executable: PathBuf,
    pub runtime_library: PathBuf,
}
const LIB_SHA: &str = "e2847c4613b386683c234913ae3b7b04299254096caf7616e3b3cd9bb97a39ab";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub version: u8,
    pub context_sha256: String,
    pub gate_rule_digest: String,
    pub descriptor_sha256: String,
    pub decoder_sha256: String,
    pub transport_sha256: String,
    pub status: String,
    pub cause: String,
    pub raw_sha256: Option<String>,
    pub raw_bytes: Option<u64>,
}
impl Status {
    pub fn binding(d: &Descriptor, c: &Context, pin: Hash, transport: Hash) -> Result<Self> {
        Ok(Self {
            version: 2,
            context_sha256: hex(c.digest()?),
            gate_rule_digest: hex(d.digest()),
            descriptor_sha256: hex(digest(&ascii(d)?)),
            decoder_sha256: hex(pin),
            transport_sha256: hex(transport),
            status: String::new(),
            cause: String::new(),
            raw_sha256: None,
            raw_bytes: None,
        })
    }
    pub fn decode(
        raw: &[u8],
        d: &Descriptor,
        c: &Context,
        pin: Hash,
        transport: Hash,
    ) -> Result<Self> {
        ensure!(raw.len() <= 8192, "decoder status cap");
        let s: Self = serde_json::from_slice(raw)?;
        ensure!(ascii(&s)? == raw, "canonical decoder status");
        let b = Self::binding(d, c, pin, transport)?;
        ensure!(
            s.version == 2
                && s.context_sha256 == b.context_sha256
                && s.gate_rule_digest == b.gate_rule_digest
                && s.descriptor_sha256 == b.descriptor_sha256
                && s.decoder_sha256 == b.decoder_sha256
                && s.transport_sha256 == b.transport_sha256,
            "decoder status binding"
        );
        hash(&s.decoder_sha256)?;
        hash(&s.transport_sha256)?;
        match s.status.as_str() {
            "ok" => {
                ensure!(
                    s.cause == "none" && s.raw_bytes.is_some_and(|n| n > 0),
                    "decoder output"
                );
                hash(
                    s.raw_sha256
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("decoder output digest"))?,
                )?;
            }
            "invalid-data" => ensure!(
                ["frame", "checksum", "digest"].contains(&s.cause.as_str())
                    && s.raw_bytes.is_none()
                    && s.raw_sha256.is_none(),
                "invalid decoder evidence"
            ),
            "unavailable" => ensure!(
                ["read", "write", "resource", "tool", "protocol"].contains(&s.cause.as_str())
                    && s.raw_bytes.is_none()
                    && s.raw_sha256.is_none(),
                "unavailable decoder evidence"
            ),
            _ => anyhow::bail!("decoder status enum"),
        }
        Ok(s)
    }
    /// Actual output is explicit; missing and contradictory evidence are local.
    pub fn decision(
        &self,
        output: Option<&[u8]>,
        context: &Context,
    ) -> Result<Option<&'static str>> {
        if self.status == "unavailable" {
            return Ok(Some("node_fault"));
        }
        if let Some(raw) = output {
            if digest(raw) != hash(&context.export_digest)? {
                return Ok(Some("input_integrity"));
            }
        }
        if self.status == "invalid-data" {
            return Ok(Some(if output.is_none() {
                "front_invalid"
            } else {
                "node_fault"
            }));
        }
        let Some(raw) = output else {
            return Ok(Some("node_fault"));
        };
        if self.raw_bytes != Some(raw.len() as u64)
            || self.raw_sha256.as_deref() != Some(hex(digest(raw)).as_str())
        {
            return Ok(Some("node_fault"));
        }
        Ok(None)
    }
}
pub struct Decoded {
    pub status: Status,
    pub raw: Option<Vec<u8>>,
    pub path: Option<PathBuf>,
    pub pin: Hash,
    pub transport: Hash,
}
fn hex(h: Hash) -> String {
    boole_derivation::attributes::hex_string(&h)[2..].into()
}
impl Config {
    pub fn verify(&self, _d: &Descriptor, deadline: Deadline) -> Result<Hash> {
        ensure!(
            cfg!(all(target_os = "macos", target_arch = "aarch64")),
            "real decoder Mac development profile only"
        );
        let release: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/devnet-m2/zstd-pin-v1.json"))?;
        let expected = hash(
            release["sha256"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("decoder release configuration"))?,
        )?;
        ensure!(
            file_hash_until(&self.executable, deadline)? == expected,
            "controller decoder release pin"
        );
        ensure!(
            file_hash_until(&self.runtime_library, deadline)? == hash(LIB_SHA)?,
            "controller decoder runtime pin"
        );
        Ok(digest(
            &[
                b"BOOLE-M3/REAL-DECODER/v1\0".as_slice(),
                &hash(LIB_SHA)?,
                &digest(include_bytes!("real_decoder.py")),
            ]
            .concat(),
        ))
    }
    #[allow(clippy::too_many_arguments)]
    pub fn decode(
        &self,
        frame: &[u8],
        length: u64,
        d: &Descriptor,
        c: &Context,
        python: &Path,
        root: &Path,
        deadline: Deadline,
    ) -> Result<Decoded> {
        self.decode_inner(
            frame,
            length,
            d,
            c,
            python,
            root,
            deadline,
            Some(hash(&c.export_digest)?),
        )
    }
    /// Package transport is authenticated by its anchored carried-byte membership,
    /// rather than the independent submission export commitment.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn decode_package(
        &self,
        frame: &[u8],
        length: u64,
        d: &Descriptor,
        c: &Context,
        python: &Path,
        root: &Path,
        deadline: Deadline,
    ) -> Result<Decoded> {
        self.decode_inner(frame, length, d, c, python, root, deadline, None)
    }
    #[allow(clippy::too_many_arguments)] // Keep input and trusted controller bindings explicit.
    fn decode_inner(
        &self,
        frame: &[u8],
        length: u64,
        d: &Descriptor,
        c: &Context,
        python: &Path,
        root: &Path,
        deadline: Deadline,
        expected: Option<Hash>,
    ) -> Result<Decoded> {
        let pin = self.verify(d, deadline)?;
        let provenance = serde_json::json!({"version":1,"decoder_sha256":hex(pin),"libzstd_sha256":LIB_SHA,"embedded_helper_sha256":hex(digest(include_bytes!("real_decoder.py"))),"interpreter_sha256":hex(file_hash_until(python, deadline)?),"legacy_cli_sha256":hex(file_hash_until(&self.executable, deadline)?)});
        deadline.write_file(
            &root.join("decoder-provenance.json"),
            &serde_json::to_vec(&provenance)?,
        )?;
        let transport = deadline.hash(frame)?;
        let mut status = Status::binding(d, c, pin, transport)?;
        let input = root.join("transport.zst");
        let output = root.join("decoded.bundle");
        if crate::codec::validate_frame(frame, length).is_err() {
            status.status = "invalid-data".into();
            status.cause = "frame".into();
            return Ok(Decoded {
                status,
                raw: None,
                path: None,
                pin,
                transport,
            });
        }
        deadline.write_file(&input, frame)?;
        // Execute against a private authenticated runtime snapshot, so a host
        // package update between verification and dlopen cannot replace it.
        let library = root.join("decoder-runtime.dylib");
        let library_bytes = deadline.read_bounded(
            &std::fs::canonicalize(&self.runtime_library)?,
            128 * 1024 * 1024,
        )?;
        ensure!(
            deadline.hash(&library_bytes)? == hash(LIB_SHA)?,
            "decoder runtime changed during snapshot"
        );
        deadline.write_file(&library, &library_bytes)?;
        let mut command = Command::new(python);
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .env("PYTHONPYCACHEPREFIX", root.join("pycache"))
            .args(["-I", "-B", "-c", include_str!("real_decoder.py")])
            .arg(&library)
            .arg(&input)
            .arg(&output)
            .arg(length.to_string())
            .arg(String::from_utf8(ascii(&status)?)?)
            .arg(expected.map(hex).unwrap_or_else(|| "package".into()));
        let result = crate::real_process::run(
            &mut command,
            &crate::process::Envelope {
                memory_mib: deadline.memory_mib().min(1024),
                wall_ms: deadline.remaining()?,
                output_cap: 8192,
            },
        )?;
        ensure!(
            result.status.success(),
            "untyped decoder process unavailable"
        );
        let status = Status::decode(&result.stdout, d, c, pin, transport)?;
        let (raw, path) = if status.status == "ok" {
            (
                Some(deadline.read_bounded(&output, length as usize)?),
                Some(output),
            )
        } else {
            (None, None)
        };
        if let Some(raw) = &raw {
            ensure!(
                status.raw_bytes == Some(raw.len() as u64)
                    && status.raw_sha256.as_deref() == Some(hex(deadline.hash(raw)?).as_str()),
                "typed output protocol mismatch"
            );
        }
        // A failed writer can leave a partial private file. It is never decoded
        // output and never used as invalid-data evidence.
        Ok(Decoded {
            status,
            raw,
            path,
            pin,
            transport,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn authenticated_package_decode_is_distinct_from_the_submission_export_commitment() -> Result<()>
    {
        let Some(path) = std::env::var_os("BOOLE_M3_REAL_DECODER") else {
            return Ok(());
        };
        let config: Config = serde_json::from_slice(&std::fs::read(path)?)?;
        let d = Descriptor::decode(include_bytes!(
            "../../../fixtures/m3-real-gate/descriptor-v1.json"
        ))?;
        let v: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../fixtures/m3-real-gate/reply-cases.json"
        ))?;
        let c: Context = serde_json::from_value(v["context"].clone())?;
        let root = crate::codec::Scratch::new(&std::env::temp_dir())?;
        let raw = b"authenticated carried problem package";
        let frame = crate::codec::Codec::new(config.executable.clone())?.compress(raw, &root.0)?;
        let out = crate::codec::Scratch::new(&root.0)?;
        let decoded = config.decode_package(
            &frame,
            raw.len() as u64,
            &d,
            &c,
            Path::new("/usr/bin/python3"),
            &out.0,
            Deadline::after(5000),
        )?;
        ensure!(decoded.status.status == "ok" && decoded.raw.as_deref() == Some(raw.as_slice()));
        let p: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.0.join("decoder-provenance.json"))?)?;
        ensure!(
            p["libzstd_sha256"] == LIB_SHA
                && p["embedded_helper_sha256"] == hex(digest(include_bytes!("real_decoder.py")))
                && p["decoder_sha256"] == hex(decoded.pin)
        );
        println!("REAL-DECODER package-purpose=true actual-lib-helper-provenance=true");
        Ok(())
    }
}
