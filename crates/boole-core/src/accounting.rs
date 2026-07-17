//! SC.4 (GAP-04) — checked accumulation for consensus accounting.
//!
//! Balance and reward folds must never wrap silently (release) or panic
//! (debug) on `u128` overflow: a would-be overflow is a typed boundary error
//! the caller propagates like any other invalid-chain rejection. Every
//! consensus and ledger accounting fold (replay, reward-ledger apply, reorg
//! recovery, receipt audit, reputation) routes its `u128` additions through
//! this one helper so they all reject overflow identically. The workspace
//! release profile also enables `overflow-checks` as a second line.

use std::collections::BTreeMap;

/// Add `amount` to `map[key]` with checked arithmetic. A `u128` overflow is a
/// typed error (never a silent wrap or panic). The message is stable for a
/// given `(key, current, amount)`, so the same overflowing chain rejects with
/// the same error across the replay / boot / reorg paths that share a fold.
pub fn checked_credit(
    map: &mut BTreeMap<String, u128>,
    key: &str,
    amount: u128,
) -> anyhow::Result<()> {
    let entry = map.entry(key.to_string()).or_insert(0);
    let current = *entry;
    *entry = current.checked_add(amount).ok_or_else(|| {
        anyhow::anyhow!(
            "consensus accounting overflow crediting {key}: {current} + {amount} exceeds u128::MAX"
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_credit_accumulates_normally() {
        let mut map = BTreeMap::new();
        checked_credit(&mut map, "pk", 10).expect("first");
        checked_credit(&mut map, "pk", 32).expect("second");
        assert_eq!(map.get("pk"), Some(&42));
    }

    #[test]
    fn checked_credit_returns_a_typed_error_on_overflow() {
        let mut map = BTreeMap::new();
        checked_credit(&mut map, "pk", u128::MAX).expect("seed to max");
        let err = checked_credit(&mut map, "pk", 1).expect_err("overflow must be an error");
        assert!(
            err.to_string().contains("consensus accounting overflow"),
            "unexpected error: {err}"
        );
        // The running balance is left unchanged on overflow (no partial wrap).
        assert_eq!(map.get("pk"), Some(&u128::MAX));
    }

    #[test]
    fn checked_credit_overflow_message_is_stable_for_the_same_inputs() {
        let mut a = BTreeMap::new();
        let mut b = BTreeMap::new();
        checked_credit(&mut a, "pk", u128::MAX).expect("seed a");
        checked_credit(&mut b, "pk", u128::MAX).expect("seed b");
        let err_a = checked_credit(&mut a, "pk", 5).expect_err("a overflow");
        let err_b = checked_credit(&mut b, "pk", 5).expect_err("b overflow");
        assert_eq!(err_a.to_string(), err_b.to_string());
    }
}
