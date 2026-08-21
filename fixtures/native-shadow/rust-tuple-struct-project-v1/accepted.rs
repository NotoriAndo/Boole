#![allow(unused)]
use crate::FixturePoint as AcfrTy;
// implement the projection in the statement
pub fn acfr_solve(items: &[AcfrTy]) -> i64 {
    // <<< ACFR-PATCH-BEGIN >>>
    let mut acc: i64 = 7;
    for it in items {
        let flag: i64 = if it.1 { 1 } else { 0 };
        let v = (it.0 as i64)
            .wrapping_mul(3)
            .wrapping_add(flag.wrapping_mul(-2));
        acc = acc.wrapping_mul(5).wrapping_add(v);
    }
    acc
    // <<< ACFR-PATCH-END >>>
}
