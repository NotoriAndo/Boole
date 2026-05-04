use num_bigint::BigUint;
use num_traits::Zero;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct CalibrationReport {
    pub T_submit: String,
    pub T_share: String,
    pub T_block: String,
    pub T_ticket: String,
    pub MinShareScoreMultiplier: f64,
    pub K_max: i64,
    pub ShareCapPerPK_Block: i64,
    pub L: i64,
    pub D_max: i64,
    pub EMAWindow: i64,
    pub M: i64,
    pub perIpRateLimitPer60s: i64,
    pub provenance: String,
}

pub fn hex_to_biguint(hex: &str) -> Result<BigUint, String> {
    let stripped = if hex.starts_with("0x") || hex.starts_with("0X") {
        &hex[2..]
    } else {
        hex
    };
    if stripped.is_empty() {
        return Err("empty hex".to_string());
    }
    BigUint::parse_bytes(stripped.as_bytes(), 16)
        .ok_or_else(|| format!("Cannot convert {hex} to a BigInt"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalibrationThresholds {
    pub t_submit: BigUint,
    pub t_share: BigUint,
    pub t_block: BigUint,
    pub t_ticket: BigUint,
}

pub fn calibration_thresholds(report: &CalibrationReport) -> Result<CalibrationThresholds, String> {
    Ok(CalibrationThresholds {
        t_submit: hex_to_biguint(&report.T_submit)?,
        t_share: hex_to_biguint(&report.T_share)?,
        t_block: hex_to_biguint(&report.T_block)?,
        t_ticket: hex_to_biguint(&report.T_ticket)?,
    })
}

pub fn validate_calibration_report(report: &CalibrationReport) -> Result<(), String> {
    let thresholds = calibration_thresholds(report)?;
    for (key, parsed) in [
        ("T_submit", &thresholds.t_submit),
        ("T_share", &thresholds.t_share),
        ("T_block", &thresholds.t_block),
        ("T_ticket", &thresholds.t_ticket),
    ] {
        if parsed.is_zero() {
            return Err(format!("{key} must be > 0"));
        }
        if *parsed > two_pow_256() {
            return Err(format!("{key} must be ≤ 2^256"));
        }
    }
    if thresholds.t_block >= thresholds.t_share {
        return Err("T_block must be strictly less than T_share".to_string());
    }
    if report.K_max <= 0 {
        return Err("K_max must be > 0".to_string());
    }
    if report.L <= 0 {
        return Err("L must be > 0".to_string());
    }
    if report.D_max <= 0 {
        return Err("D_max must be > 0".to_string());
    }
    if report.MinShareScoreMultiplier <= 0.0 {
        return Err("MinShareScoreMultiplier must be > 0".to_string());
    }
    Ok(())
}

fn two_pow_256() -> BigUint {
    BigUint::from(1u8) << 256usize
}
