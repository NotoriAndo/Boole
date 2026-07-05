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
//   - `FixedSeedTargetEmitter` — pin a known seed/render pair for smoke tests.
//   - `FamilyV1LengthBoundTargetEmitter` — active production path. Derives
//     the seed via `target_seed(...)`, generates the v1 length-bound instance,
//     and renders the theorem statement directly. No external Lake call required.
use boole_core::Hex32;

use crate::canonicalizer::Target;
use crate::family_v1_lenbound::{
    generate_from_hex as generate_v1_lenbound_from_hex, render_text as render_v1_lenbound_text,
};

// The derivation itself lives in `boole_core::target_seed` — the single
// shared function admission and replay use to re-derive a claimed seed —
// re-exported here so the miner keeps one import path for it.
pub use boole_core::target_seed;

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

/// Pin a single (seed, render) pair regardless of `(c, pk, n, j)`. Used
/// for v01-style smoke runs where the proof and render text are
/// hand-written.
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

/// Production target emitter for the v1-lenbound capability calibration profile.
pub struct FamilyV1LengthBoundTargetEmitter {
    pub pinned_seed_hex: Option<String>,
}

impl FamilyV1LengthBoundTargetEmitter {
    pub fn new() -> Self {
        Self {
            pinned_seed_hex: None,
        }
    }

    pub fn with_pinned_seed(mut self, seed_hex: impl Into<String>) -> Self {
        self.pinned_seed_hex = Some(seed_hex.into());
        self
    }
}

impl Default for FamilyV1LengthBoundTargetEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl TargetEmitter for FamilyV1LengthBoundTargetEmitter {
    fn emit(&self, args: &TargetEmitArgs<'_>) -> anyhow::Result<Target> {
        let seed_hex = match &self.pinned_seed_hex {
            Some(s) => s.clone(),
            None => target_seed(args.c, args.pk, args.n, args.j_index).to_hex(),
        };
        let instance = generate_v1_lenbound_from_hex(&seed_hex)
            .map_err(|e| anyhow::anyhow!("decode seed_hex: {e}"))?;
        let render = render_v1_lenbound_text(&instance);
        Ok(Target {
            seed_hex,
            d: args.d,
            profile: "v1-lenbound".to_string(),
            n: args.n_param.unwrap_or(1),
            render,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_args() -> (Hex32, Hex32, Hex32) {
        (
            Hex32::from_bytes([0u8; 32]),
            Hex32::from_bytes([1u8; 32]),
            Hex32::from_bytes([2u8; 32]),
        )
    }

    #[test]
    fn family_emitter_derives_seed_and_render() {
        let (c, pk, n) = dummy_args();
        let emitter = FamilyV1LengthBoundTargetEmitter::new();
        let args = TargetEmitArgs {
            c: &c,
            pk: &pk,
            n: &n,
            j_index: 0,
            d: 1,
            profile: "v1-lenbound".to_string(),
            n_param: Some(1),
        };
        let target = emitter.emit(&args).expect("emit");
        assert_eq!(target.profile, "v1-lenbound");
        assert_eq!(target.seed_hex.len(), 64);
        assert!(target
            .render
            .starts_with("theorem instance_thm : ∀ (xs : List Int),"));
    }

    #[test]
    fn family_emitter_pinned_seed_overrides_derivation() {
        let (c, pk, n) = dummy_args();
        let pinned = "b606f7037936d8191ded73d7051fb423e72d2b442b0e868da9e3b11e72c7f764";
        let emitter = FamilyV1LengthBoundTargetEmitter::new().with_pinned_seed(pinned);
        let args = TargetEmitArgs {
            c: &c,
            pk: &pk,
            n: &n,
            j_index: 5,
            d: 0,
            profile: "v1-lenbound".to_string(),
            n_param: None,
        };
        let target = emitter.emit(&args).expect("emit");
        assert_eq!(target.seed_hex, pinned);
    }

    #[test]
    fn family_emitter_rejects_bad_hex() {
        let (c, pk, n) = dummy_args();
        let emitter = FamilyV1LengthBoundTargetEmitter::new().with_pinned_seed("not-hex");
        let args = TargetEmitArgs {
            c: &c,
            pk: &pk,
            n: &n,
            j_index: 0,
            d: 0,
            profile: "v1-lenbound".to_string(),
            n_param: None,
        };
        assert!(emitter.emit(&args).is_err());
    }

    #[test]
    fn target_seed_changes_with_j() {
        let (c, pk, n) = dummy_args();
        let s0 = target_seed(&c, &pk, &n, 0);
        let s1 = target_seed(&c, &pk, &n, 1);
        assert_ne!(s0.to_hex(), s1.to_hex());
    }
}
