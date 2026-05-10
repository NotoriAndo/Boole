// Target emitter — derives the (seed, render) pair the LLM is asked to prove.
//
// The seed is `H_protocol("target", c, pk, n, j_be4)` where `j_be4` is the
// 4-byte big-endian encoding of the per-cycle index `j ∈ [0, M)`. This
// matches pof's `targetGen.ts::targetSeed` byte-for-byte: the dispatcher
// can re-derive the same seed from on-chain `(c, pk, n, j_index)` to verify
// a miner's claimed target.
//
// The Boole grinder's `j` (in `share_hash`) is a 32-byte Hex32 nonce — that
// is a *different* j from the target index. The target index is the loop
// counter 0..M; the share j is grinded.
//
// Three implementations:
//   - `StubTargetEmitter` — synthetic in-memory render for tests.
//   - `FixedSeedTargetEmitter` — pin a known seed/render pair (for
//     deterministic real-verifier smoke runs).
//   - `LakeTargetEmitter` (feature `lake-target`) — production path that
//     shells out to `lake exec gen_target_emit`.
use boole_core::{h_protocol, Hex32};

use crate::canonicalizer::Target;

const DOMAIN_TARGET: &[u8] = b"target";

/// Compute the deterministic target seed for `(c, pk, n, j_index)`. The
/// `j_index` is the integer loop counter 0..M, NOT the 32-byte share j.
pub fn target_seed(c: &Hex32, pk: &Hex32, n: &Hex32, j_index: u32) -> Hex32 {
    let j_be = j_index.to_be_bytes();
    h_protocol(
        DOMAIN_TARGET,
        &[c.as_bytes(), pk.as_bytes(), n.as_bytes(), &j_be],
    )
}

#[derive(Debug, Clone)]
pub struct TargetEmitArgs<'a> {
    pub c: &'a Hex32,
    pub pk: &'a Hex32,
    pub n: &'a Hex32,
    pub j_index: u32,
    pub d: u32,
    pub profile: String,
    pub n_param: Option<u32>,
}

pub trait TargetEmitter: Send + Sync {
    fn emit(&self, args: &TargetEmitArgs<'_>) -> anyhow::Result<Target>;
}

/// In-memory stub emitter: returns the same render text for every j, with
/// the seed derived from the protocol hash. Useful for unit tests where
/// we only care that the loop iterates and that distinct j values produce
/// distinct seeds.
pub struct StubTargetEmitter {
    render: String,
}

impl StubTargetEmitter {
    pub fn new(render: impl Into<String>) -> Self {
        Self {
            render: render.into(),
        }
    }
}

impl TargetEmitter for StubTargetEmitter {
    fn emit(&self, args: &TargetEmitArgs<'_>) -> anyhow::Result<Target> {
        let seed = target_seed(args.c, args.pk, args.n, args.j_index);
        Ok(Target {
            seed_hex: seed.to_hex(),
            d: args.d,
            profile: args.profile.clone(),
            n: args.n_param.unwrap_or(1),
            render: self.render.clone(),
        })
    }
}

/// Pin a single (seed, render) pair regardless of `(c, pk, n, j)`. Used for
/// deterministic smoke runs against the real verifier where the proof is
/// known in advance.
#[derive(Debug, Clone)]
pub struct FixedSeedTargetEmitter {
    pub seed_hex: String,
    pub render: String,
    pub d: u32,
    pub profile: String,
    pub n: Option<u32>,
}

impl TargetEmitter for FixedSeedTargetEmitter {
    fn emit(&self, args: &TargetEmitArgs<'_>) -> anyhow::Result<Target> {
        Ok(Target {
            seed_hex: self.seed_hex.clone(),
            d: self.d,
            profile: self.profile.clone(),
            n: self.n.or(args.n_param).unwrap_or(1),
            render: self.render.clone(),
        })
    }
}

#[cfg(feature = "lake-target")]
mod lake {
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::Duration;

    use super::{target_seed, Target, TargetEmitArgs, TargetEmitter};

    /// Production target emitter. Invokes:
    ///   lake exec gen_target_emit <seed_hex> <D> <out_path> <profile> [<N>]
    /// in `lean_dir` and reads the produced render text from `out_path`.
    pub struct LakeTargetEmitter {
        pub lean_dir: PathBuf,
        pub timeout: Duration,
    }

    impl LakeTargetEmitter {
        pub fn new(lean_dir: PathBuf) -> Self {
            Self {
                lean_dir,
                timeout: Duration::from_secs(60),
            }
        }
    }

    impl TargetEmitter for LakeTargetEmitter {
        fn emit(&self, args: &TargetEmitArgs<'_>) -> anyhow::Result<Target> {
            let tmp = std::env::temp_dir().join(format!(
                "boole-target-{}-{}.txt",
                std::process::id(),
                args.j_index,
            ));
            let seed = target_seed(args.c, args.pk, args.n, args.j_index);
            let seed_hex = seed.to_hex();
            let mut cmd = Command::new("lake");
            cmd.arg("exec")
                .arg("gen_target_emit")
                .arg(&seed_hex)
                .arg(args.d.to_string())
                .arg(&tmp)
                .arg(&args.profile);
            if matches!(args.profile.as_str(), "v03" | "v031" | "v031-lp") {
                cmd.arg(args.n_param.unwrap_or(1).to_string());
            }
            cmd.current_dir(&self.lean_dir);
            let out = cmd
                .output()
                .map_err(|e| anyhow::anyhow!("lake exec failed: {e}"))?;
            if !out.status.success() {
                let _ = std::fs::remove_file(&tmp);
                return Err(anyhow::anyhow!(
                    "lake exec gen_target_emit exited {}: {}",
                    out.status,
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
            let render = std::fs::read_to_string(&tmp)?;
            let _ = std::fs::remove_file(&tmp);
            let _ = self.timeout; // reserved for a future timeout-aware spawner
            Ok(Target {
                seed_hex,
                d: args.d,
                profile: args.profile.clone(),
                n: args.n_param.unwrap_or(1),
                render,
            })
        }
    }
}

#[cfg(feature = "lake-target")]
pub use lake::LakeTargetEmitter;
