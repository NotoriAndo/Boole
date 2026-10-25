//! Node-local retention; no pin byte is a consensus input.
use anyhow::Result;
#[derive(Clone, Copy, Debug)]
pub struct RetentionPolicy {
    pub enabled: bool,
}
impl Default for RetentionPolicy {
    fn default() -> Self {
        Self { enabled: true }
    }
}
impl RetentionPolicy {
    pub fn verify_mode(self) -> Result<()> {
        if !self.enabled {
            return Err(crate::error::startup(
                20,
                "verify requires durable full retention",
            ));
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t70_s20_verify_refuses_retention_off() {
        assert!(RetentionPolicy::default().verify_mode().is_ok());
        let e = RetentionPolicy { enabled: false }
            .verify_mode()
            .expect_err("verify started without retention");
        assert!(matches!(
            e.downcast_ref::<crate::error::Error>(),
            Some(crate::error::Error::Startup { number: 20, .. })
        ));
    }
}
mod model;
pub use model::*;
mod storage;
pub use storage::*;
