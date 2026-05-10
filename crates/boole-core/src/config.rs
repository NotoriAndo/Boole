use num_bigint::BigUint;
use num_traits::Zero;
use serde::{Deserialize, Serialize};
use serde_json::Number;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct CalibrationReport {
    pub T_submit: String,
    pub T_share: String,
    pub T_block: String,
    pub T_ticket: String,
    pub MinShareScoreMultiplier: Number,
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

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationPolicy {
    pub thresholds: CalibrationThresholds,
    pub k_max: usize,
    pub share_cap_per_pk_block: usize,
    pub global_share_cap: usize,
    pub l: usize,
    pub d_max: usize,
    pub m: i64,
    pub per_ip_rate_limit_per_60s: usize,
    pub min_share_score_multiplier_nanos: u64,
}

fn min_share_score_multiplier_nanos(report: &CalibrationReport) -> Result<u64, String> {
    parse_decimal_nanos(&report.MinShareScoreMultiplier.to_string())
}

/// Parse a non-negative decimal numeral (e.g. "1", "1.0", "0.5") into a
/// fixed-point u64 measured in nanos (10⁻⁹). Used by both the calibration
/// validator and downstream consumers (e.g. `boole-miner`'s `/head` parser)
/// that need to read `MinShareScoreMultiplier`-style decimal fields without
/// duplicating this logic.
pub fn parse_decimal_nanos(raw: &str) -> Result<u64, String> {
    if raw.starts_with('-') {
        return Err("MinShareScoreMultiplier must be > 0".to_string());
    }
    let (whole, fractional) = match raw.split_once('.') {
        Some((whole, fractional)) => (whole, fractional),
        None => (raw, ""),
    };
    if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("MinShareScoreMultiplier must be decimal".to_string());
    }
    if !fractional.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("MinShareScoreMultiplier must be decimal".to_string());
    }
    if fractional.len() > 9 {
        return Err("MinShareScoreMultiplier must have at most 9 decimal places".to_string());
    }

    let whole_nanos = whole
        .parse::<u64>()
        .map_err(|_| "MinShareScoreMultiplier is too large".to_string())?
        .checked_mul(1_000_000_000)
        .ok_or_else(|| "MinShareScoreMultiplier is too large".to_string())?;
    let mut frac = fractional.to_string();
    while frac.len() < 9 {
        frac.push('0');
    }
    let fractional_nanos = if frac.is_empty() {
        0
    } else {
        frac.parse::<u64>()
            .map_err(|_| "MinShareScoreMultiplier must be decimal".to_string())?
    };
    let nanos = whole_nanos
        .checked_add(fractional_nanos)
        .ok_or_else(|| "MinShareScoreMultiplier is too large".to_string())?;
    if nanos == 0 {
        return Err("MinShareScoreMultiplier must be > 0".to_string());
    }
    Ok(nanos)
}

pub fn calibration_policy(report: &CalibrationReport) -> Result<CalibrationPolicy, String> {
    validate_calibration_report(report)?;
    Ok(CalibrationPolicy {
        thresholds: calibration_thresholds(report)?,
        k_max: report.K_max as usize,
        share_cap_per_pk_block: report.ShareCapPerPK_Block as usize,
        global_share_cap: report.K_max as usize,
        l: report.L as usize,
        d_max: report.D_max as usize,
        m: report.M,
        per_ip_rate_limit_per_60s: report.perIpRateLimitPer60s as usize,
        min_share_score_multiplier_nanos: min_share_score_multiplier_nanos(report)?,
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
    if report.M <= 0 {
        return Err("M must be > 0".to_string());
    }
    if report.perIpRateLimitPer60s <= 0 {
        return Err("perIpRateLimitPer60s must be > 0".to_string());
    }
    if report.ShareCapPerPK_Block <= 0 {
        return Err("ShareCapPerPK_Block must be > 0".to_string());
    }
    min_share_score_multiplier_nanos(report)?;
    Ok(())
}

fn two_pow_256() -> BigUint {
    BigUint::from(1u8) << 256usize
}
