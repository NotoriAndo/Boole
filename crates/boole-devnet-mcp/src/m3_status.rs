//! Public progress only. Frozen B, salt, parts and worker inputs never enter a view.
use anyhow::{Context, Result};
use boole_derivation::{
    attributes::hex_string,
    m2::{Config, Key, State, Status},
    m3::*,
};
use serde_json::{json, Map, Value};

pub(crate) fn reward(c: &Config, r: &Reward) -> Value {
    let vesting_status = match r.status {
        RewardStatus::Locked => "LOCKED",
        RewardStatus::Vested => "VESTED",
        RewardStatus::Cancelled => "CANCELLED",
    };
    let status = if r.compensation && r.status == RewardStatus::Locked {
        "COMPENSATION"
    } else {
        vesting_status
    };
    json!({"reward_id":r.id,"problem_id":hex_string(&r.key.0),"submission_id":hex_string(&r.key.1),"payee":hex_string(&r.solver),"amount":r.amount.to_string(),"vest_h":r.vest_h,"status":status,"vesting_status":vesting_status,"compensation":r.compensation,"payment_emitted":r.status==RewardStatus::Vested && c.m3.as_ref().is_some_and(|p|p.mint_enabled)})
}
pub(crate) fn submission(s: &State, c: &Config, key: Key) -> Result<Map<String, Value>> {
    let mut value = json!({"queue_index":null,"earliest_due_height":null,"verdict":null,"earlier_candidates":[],"pending_recheck":false,"rewards":[]});
    if let Some(m) = &s.m3 {
        if let Some(job) = m
            .queue
            .iter()
            .find(|j| j.key == key && j.kind == RecordKind::Verdict)
        {
            value["queue_index"] = json!(job.queue_index);
            value["earliest_due_height"] = json!(job
                .avail
                .checked_add(c.m3.as_ref().context("M3 profile")?.k)
                .context("earliest due overflow")?);
        }
        if let Some(v) = m.results.get(&key) {
            let status = match v.outcome {
                Outcome::Accept => "ACCEPT",
                Outcome::Reject => "REJECT",
                Outcome::Void => "VOID",
            };
            value["verdict"] = json!({"status":status,"reason":v.reason,"fuel":v.fuel,"epoch":v.gate_epoch,"kind":v.kind as u8,"vstmt_digest":hex_string(&v.vstmt_digest)});
        }
        value["pending_recheck"] = json!(m.pending_rechecks.contains(&key));
        let unresolved = |status| {
            matches!(
                status,
                Status::Committed | Status::Open | Status::Checking | Status::Passed
            )
        };
        if s.candidates
            .get(&key)
            .is_some_and(|r| unresolved(r.status()))
            && s.problems.get(&key.0).is_some_and(|p| p.winner.is_none())
        {
            let earlier:Vec<_>=s.priority(key.0).into_iter().take_while(|ch|*ch!=key.1).filter_map(|ch| {
                let candidate=s.candidates.get(&(key.0,ch))?;
                if !unresolved(candidate.status()) {return None;}
                let live=candidate.live()?;
                let status=match candidate.status() {Status::Committed=>"COMMITTED",Status::Open=>"OPEN",Status::Checking=>"CHECKING",Status::Passed=>"PASSED",_=>unreachable!()};
                Some(json!({"submission_id":hex_string(&ch),"status":status,"commit_height":live.h_c,"queue_index":live.open.as_ref().and_then(|o|o.queue_index),"pending_recheck":m.pending_rechecks.contains(&(key.0,ch))}))
            }).collect();
            value["earlier_candidates"] = json!(earlier);
        }
        value["rewards"] = json!(m
            .rewards
            .values()
            .filter(|r| r.key == key)
            .map(|r| reward(c, r))
            .collect::<Vec<_>>());
    }
    Ok(value.as_object().expect("view object").clone())
}
pub(crate) fn summary(s: &State) -> Value {
    s.m3.as_ref().map(|m|json!({"queue_entries":m.queue.len(),"next_unapplied_queue_index":m.queue.get(m.next_applied).map(|j|j.queue_index),"checking":s.candidates.values().filter(|c|c.status()==Status::Checking).count(),"passed":s.candidates.values().filter(|c|c.status()==Status::Passed).count(),"accepted":s.candidates.values().filter(|c|c.status()==Status::Accepted).count(),"pending_rechecks":m.pending_rechecks.len(),"locked_rewards":m.rewards.values().filter(|r|r.status==RewardStatus::Locked).count(),"vested_rewards":m.rewards.values().filter(|r|r.status==RewardStatus::Vested).count()})).unwrap_or(Value::Null)
}

pub(crate) fn quantity_word(value: &str) -> Result<[u8; 32]> {
    let digits = value
        .strip_prefix("0x")
        .context("balance quantity prefix")?;
    anyhow::ensure!(
        !digits.is_empty() && digits.len() <= 64 && (digits.len() == 1 || !digits.starts_with('0')),
        "balance quantity length/canonical form"
    );
    let mut word = [0; 32];
    for (i, b) in digits.bytes().rev().enumerate() {
        let nibble = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => anyhow::bail!("balance quantity digit"),
        };
        word[31 - i / 2] |= nibble << (4 * (i % 2));
    }
    Ok(word)
}
pub(crate) fn word_hex(word: &[u8; 32]) -> String {
    let hex = hex_string(word);
    let digits = hex[2..].trim_start_matches('0');
    format!("0x{}", if digits.is_empty() { "0" } else { digits })
}
pub(crate) fn balance_delta(before: &[u8; 32], after: &[u8; 32]) -> String {
    let negative = after < before;
    let (high, low) = if negative {
        (before, after)
    } else {
        (after, before)
    };
    let mut delta = [0; 32];
    let mut borrow = 0i16;
    for i in (0..32).rev() {
        let d = i16::from(high[i]) - i16::from(low[i]) - borrow;
        delta[i] = d.rem_euclid(256) as u8;
        borrow = i16::from(d < 0);
    }
    format!("{}{}", if negative { "-" } else { "" }, word_hex(&delta))
}
