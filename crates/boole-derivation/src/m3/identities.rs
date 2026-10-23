use super::{validate_reason, Error, Outcome};
use crate::m2::Hash;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictStatement {
    pub chain_id: u64,
    pub problem_id: Hash,
    pub commit_hash: Hash,
    pub solver: [u8; 20],
    pub env_id: Hash,
    pub export_digest: Hash,
    pub statement_id: Hash,
    pub gate_rule_digest: Hash,
    pub kernel_set_digest: Hash,
    pub outcome: Outcome,
    pub reason: u16,
    pub ext: Vec<u8>,
}
impl VerdictStatement {
    pub fn preimage(&self, lane: u8) -> Result<Vec<u8>, Error> {
        validate_reason(self.outcome, self.reason)?;
        if ![1, 2].contains(&lane) {
            return Err(Error::Format("statement lane"));
        }
        if self.outcome != Outcome::Reject && self.statement_id == [0; 32] {
            return Err(Error::Format("missing statement identity"));
        }
        if self.outcome == Outcome::Accept && lane == 2 {
            if self.ext.len() < 66 || self.ext.len() != 66 + 8 * usize::from(self.ext[1]) {
                return Err(Error::Format("lane2 extension"));
            }
        } else if !self.ext.is_empty() {
            return Err(Error::Format("unexpected statement extension"));
        }
        let n = u16::try_from(self.ext.len()).map_err(|_| Error::Overflow)?;
        let mut out = b"BOOLE-M3/VSTMT/v1".to_vec();
        out.extend(self.chain_id.to_be_bytes());
        out.extend(self.problem_id);
        out.extend(self.commit_hash);
        out.extend(self.solver);
        out.extend(self.env_id);
        out.extend(self.export_digest);
        out.extend(self.statement_id);
        out.extend(self.gate_rule_digest);
        out.extend(self.kernel_set_digest);
        out.push(self.outcome as u8);
        out.extend(self.reason.to_be_bytes());
        out.extend(n.to_be_bytes());
        out.extend(&self.ext);
        Ok(out)
    }
    pub fn digest(&self, lane: u8) -> Result<Hash, Error> {
        Ok(Sha256::digest(self.preimage(lane)?).into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub name: Vec<u8>,
    pub kind: u8,
    pub decl_hash: Hash,
}

/// Hash already alpha-v1-normalized declaration identities; this is not a Lean parser/builder.
pub fn manifest_digest(declarations: &[Declaration]) -> Result<Hash, Error> {
    let mut sorted: Vec<_> = declarations.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    if sorted.windows(2).any(|w| w[0].name == w[1].name) {
        return Err(Error::Format("duplicate declaration"));
    }
    let mut out = b"BOOLE-M3/MANIFEST/v1".to_vec();
    out.extend(
        u32::try_from(sorted.len())
            .map_err(|_| Error::Overflow)?
            .to_be_bytes(),
    );
    for d in sorted {
        out.extend(
            u16::try_from(d.name.len())
                .map_err(|_| Error::Overflow)?
                .to_be_bytes(),
        );
        out.extend(&d.name);
        out.push(d.kind);
        out.extend(d.decl_hash);
    }
    Ok(Sha256::digest(out).into())
}
