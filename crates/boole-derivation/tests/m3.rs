//! M3a: consensus behavior through Config, State and canonical deposit APIs.
use boole_derivation::m2::Config;
use boole_derivation::m3::M3Config;
#[path = "support/m3.rs"]
mod scenario;

fn config() -> Config {
    let mut c = Config::devnet();
    c.m3 = Some(M3Config {
        k: 8,
        rate: 1000,
        cost_fixed: 100,
        fuel_slot: vec![10000; 2],
        verdict_sink: [
            0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xb3,
        ],
        q_reward: 16,
        reward_gas: 100000,
        maturity: 64,
        reward: vec![1; 2],
        mint_enabled: true,
        notice: 0,
        open_licenses: [1].into(),
        bridge: false,
        native_is_reward_unit: false,
        genesis_gate_rule_digest: [7; 32],
    });
    c
}

#[test]
fn t40_m3_profile_is_staged_and_legacy_config_bytes_stay_identical() {
    let old = Config::devnet();
    let before = serde_json::to_vec(&old).unwrap();
    assert!(!String::from_utf8_lossy(&before).contains("\"m3\""));
    let mut staged: serde_json::Value = serde_json::from_slice(&before).unwrap();
    staged["m3"] = serde_json::json!({
        "k":8,"rate":1000,"cost_fixed":100,"fuel_slot":[10000,10000],
        "verdict_sink":[66,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,179],
        "q_reward":16,"reward_gas":100000,"maturity":64,"reward":[1,1],
        "mint_enabled":true,"notice":0,"open_licenses":[1],
        "bridge":false,"native_is_reward_unit":false,
        "genesis_gate_rule_digest":vec![7;32]
    });
    let decoded = serde_json::from_value::<Config>(staged);
    assert!(
        decoded.is_ok(),
        "valid optional M3 profile must be accepted: {decoded:?}"
    );
    let on = decoded.unwrap();
    assert_ne!(on.digest(), old.digest());
    assert_eq!(on.validate(30_000_000), Ok(()));
    assert_eq!(serde_json::to_vec(&old).unwrap(), before);
}

#[test]
fn t40_s9_s15_s17_refuse_invalid_profiles_at_exact_boundaries() {
    let c = config();
    let mut cases = vec![];
    let mut bad = c.clone();
    bad.verdict_gas = 431_559;
    cases.push((bad, 30_000_000, 9));
    let mut bad = c.clone();
    bad.m3.as_mut().unwrap().reward_gas = 20_999;
    cases.push((bad, 30_000_000, 9));
    cases.push((c.clone(), 28_377_215, 10));
    for which in 0..6 {
        let mut bad = c.clone();
        let p = bad.m3.as_mut().unwrap();
        match which {
            0 => p.k = 0,
            1 => p.rate = 0,
            2 => p.cost_fixed = 0,
            3 => p.fuel_slot.pop().map(|_| ()).unwrap(),
            4 => p.fuel_slot[0] = 0,
            _ => p.fuel_slot[0] = (1u64 << 63) - p.cost_fixed + 1,
        };
        cases.push((bad, 30_000_000, 11));
    }
    let mut bad = c.clone();
    bad.m3.as_mut().unwrap().maturity = 15;
    cases.push((bad, 30_000_000, 12));
    let mut bad = c.clone();
    bad.m3.as_mut().unwrap().notice = 57;
    cases.push((bad, 30_000_000, 12));
    let mut bad = c.clone();
    bad.m3.as_mut().unwrap().reward.pop();
    cases.push((bad, 30_000_000, 13));
    let mut bad = c.clone();
    bad.m3.as_mut().unwrap().reward[0] = 0;
    cases.push((bad, 30_000_000, 13));
    let mut bad = c.clone();
    bad.m3.as_mut().unwrap().open_licenses.clear();
    cases.push((bad, 30_000_000, 15));
    let mut bad = c.clone();
    bad.m3.as_mut().unwrap().bridge = true;
    cases.push((bad, 30_000_000, 17));
    let errors: Vec<_> = cases
        .into_iter()
        .enumerate()
        .filter_map(|(i, (c, gas, s))| {
            let got = c.validate(gas);
            (got != Err(s)).then_some((i, s, got))
        })
        .collect();
    assert!(errors.is_empty(), "startup checks not enforced: {errors:?}");
    assert_eq!(c.validate(28_377_216), Ok(()));
    let mut off = c;
    let p = off.m3.as_mut().unwrap();
    p.mint_enabled = false;
    p.reward.fill(0);
    p.bridge = true;
    assert_eq!(off.validate(30_000_000), Ok(()));
}

#[test]
fn t47_verdict_and_reward_bytes_match_independent_python_and_roundtrip() {
    use boole_derivation::{m3::*, L1Block};
    let vector: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/devnet-m3/system-v1.3.json")).unwrap();
    assert_eq!(
        vector["provenance"]["generator_sha256"],
        hex::encode(scenario::hash(include_bytes!(
            "../../../scripts/devnet/generate_m3_vectors.py"
        )))
    );
    let l1 = L1Block {
        number: 7,
        hash: format!("0x{}", vector["origin"].as_str().unwrap()),
        parent_hash: String::new(),
        timestamp: 7,
        mix_hash: String::new(),
        base_fee_per_gas: String::new(),
        parent_beacon_block_root: String::new(),
    };
    let c = config();
    let records: Vec<_> = vector["records"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, x)| IndexedVerdict {
            queue_index: 41 + i as u64,
            record: VerdictRecord::decode(&hex::decode(x.as_str().unwrap()).unwrap()).unwrap(),
        })
        .collect();
    let encoded = verdict_deposits(&c, &l1, &records).unwrap();
    let expected: Vec<_> = vector["verdict_deposits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| format!("0x{}", x.as_str().unwrap()))
        .collect();
    assert_eq!(encoded, expected);
    assert_eq!(decode_verdict_deposits(&c, &l1, &encoded).unwrap(), records);
    assert!(verdict_deposits(&c, &l1, &[]).unwrap().is_empty());
    let payment = RewardPayment {
        reward_id: 9,
        solver: [17; 20],
        amount: 1_000_000_000_000_000_000_000,
    };
    let raw = reward_deposit(&c, &l1, &payment).unwrap();
    assert_eq!(
        raw,
        format!("0x{}", vector["reward"]["deposit"].as_str().unwrap())
    );
    assert_eq!(decode_reward_deposit(&c, &l1, 9, &raw).unwrap(), payment);
}

fn wire_input() -> (Config, boole_derivation::L1Block, Vec<String>, String) {
    let v: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/devnet-m3/system-v1.3.json")).unwrap();
    let l1 = boole_derivation::L1Block {
        number: 7,
        hash: format!("0x{}", v["origin"].as_str().unwrap()),
        parent_hash: String::new(),
        timestamp: 7,
        mix_hash: String::new(),
        base_fee_per_gas: String::new(),
        parent_beacon_block_root: String::new(),
    };
    let verdicts = v["verdict_deposits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| format!("0x{}", x.as_str().unwrap()))
        .collect();
    (
        config(),
        l1,
        verdicts,
        format!("0x{}", v["reward"]["deposit"].as_str().unwrap()),
    )
}
fn change_field(tx: &str, field: usize, new: Option<Vec<u8>>) -> String {
    let bytes = hex::decode(&tx[2..]).unwrap();
    let decoded = boole_derivation::rlp::decode(&bytes[1..]).unwrap();
    let mut fields: Vec<_> = decoded
        .children
        .unwrap()
        .into_iter()
        .map(|x| x.bytes.to_vec())
        .collect();
    if let Some(new) = new {
        fields[field] = new;
    } else {
        fields[field][0] ^= 1;
    }
    let encoded: Vec<_> = fields
        .iter()
        .map(|x| boole_derivation::rlp::bytes(x))
        .collect();
    format!("0x7e{}", hex::encode(boole_derivation::rlp::list(&encoded)))
}
#[test]
fn t47_strict_decoder_refuses_changed_fields_missing_reordered_and_partial_batches() {
    use boole_derivation::m3::*;
    let (c, l1, valid, reward) = wire_input();
    let mut accepted = vec![];
    for field in 0..8 {
        let mut bad = valid.clone();
        bad[0] = change_field(
            &bad[0],
            field,
            if matches!(field, 3 | 4 | 6) {
                Some(vec![1])
            } else {
                None
            },
        );
        if decode_verdict_deposits(&c, &l1, &bad).is_ok() {
            accepted.push(format!("VERDICT field{field}"));
        }
        let bad = change_field(
            &reward,
            field,
            if field == 6 || field == 7 {
                Some(vec![1])
            } else {
                None
            },
        );
        let expected = RewardPayment {
            reward_id: 9,
            solver: [17; 20],
            amount: 1_000_000_000_000_000_000_000,
        };
        if check_reward_deposit(&c, &l1, &expected, &bad).is_ok() {
            accepted.push(format!("REWARD field{field}"));
        }
    }
    let raw = hex::decode(&valid[0][2..]).unwrap();
    let f = boole_derivation::rlp::decode(&raw[1..])
        .unwrap()
        .children
        .unwrap();
    for at in [12, 13, 21] {
        let mut data = f[7].bytes.to_vec();
        data[at] ^= 1;
        let mut bad = valid.clone();
        bad[0] = change_field(&bad[0], 7, Some(data));
        if decode_verdict_deposits(&c, &l1, &bad).is_ok() {
            accepted.push(format!("header byte{at}"));
        }
    }
    let mut partial = f[7].bytes.to_vec();
    partial.truncate(partial.len() - 80);
    partial[22..24].copy_from_slice(&127u16.to_be_bytes());
    let cases = [
        vec![valid[0].clone()],
        vec![valid[1].clone()],
        vec![valid[1].clone(), valid[0].clone()],
        vec![valid[0].clone(), valid[1].clone(), valid[1].clone()],
        vec![change_field(&valid[0], 7, Some(partial)), valid[1].clone()],
    ];
    for (i, bad) in cases.iter().enumerate() {
        if decode_verdict_deposits(&c, &l1, bad).is_ok() {
            accepted.push(format!("batch case{i}"));
        }
    }
    if decode_reward_deposit(&c, &l1, 10, &reward).is_ok() {
        accepted.push("reward id".into());
    }
    assert!(
        accepted.is_empty(),
        "malformed deposits accepted: {accepted:?}"
    );
}

#[test]
fn t48_vstmt_and_t62_manifest_match_independent_goldens() {
    use boole_derivation::m3::*;
    let v: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/devnet-m3/system-v1.3.json")).unwrap();
    for row in v["statements"].as_array().unwrap() {
        let statement = VerdictStatement {
            chain_id: 720926,
            problem_id: [1; 32],
            commit_hash: [2; 32],
            solver: [3; 20],
            env_id: [4; 32],
            export_digest: [5; 32],
            statement_id: [6; 32],
            gate_rule_digest: [7; 32],
            kernel_set_digest: [8; 32],
            outcome: Outcome::Accept,
            reason: 0,
            ext: hex::decode(row["ext"].as_str().unwrap()).unwrap(),
        };
        assert_eq!(
            hex::encode(
                statement
                    .preimage(row["lane"].as_u64().unwrap() as u8)
                    .unwrap()
            ),
            row["preimage"].as_str().unwrap()
        );
        assert_eq!(
            hex::encode(
                statement
                    .digest(row["lane"].as_u64().unwrap() as u8)
                    .unwrap()
            ),
            row["digest"].as_str().unwrap()
        );
        let original = statement
            .digest(row["lane"].as_u64().unwrap() as u8)
            .unwrap();
        for field in 0..10 {
            let mut other = statement.clone();
            match field {
                0 => other.chain_id += 1,
                1 => other.problem_id[0] ^= 1,
                2 => other.commit_hash[0] ^= 1,
                3 => other.solver[0] ^= 1,
                4 => other.env_id[0] ^= 1,
                5 => other.export_digest[0] ^= 1,
                6 => other.statement_id[0] ^= 1,
                7 => other.gate_rule_digest[0] ^= 1,
                8 => other.kernel_set_digest[0] ^= 1,
                _ => {
                    other.outcome = Outcome::Reject;
                    other.reason = 0x0101;
                    other.ext.clear();
                }
            }
            assert_ne!(
                other.digest(row["lane"].as_u64().unwrap() as u8).unwrap(),
                original
            );
        }
    }
    let declarations: Vec<_> = v["manifest"]["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| Declaration {
            name: x["name"].as_str().unwrap().as_bytes().to_vec(),
            kind: x["kind"].as_u64().unwrap() as u8,
            decl_hash: hex::decode(x["decl_hash"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap(),
        })
        .collect();
    assert_eq!(
        hex::encode(manifest_digest(&declarations).unwrap()),
        v["manifest"]["digest"].as_str().unwrap()
    );
    let mut reversed = declarations.clone();
    reversed.reverse();
    assert_eq!(manifest_digest(&declarations), manifest_digest(&reversed));
}

#[test]
fn t47_complete_system_prefix_and_local_verdicts_are_mandatory() {
    use boole_derivation::m3::*;
    let (c, l1, verdicts, reward) = wire_input();
    let prefix = vec![
        "0x7e01".into(),
        "0x7e02".into(),
        verdicts[0].clone(),
        verdicts[1].clone(),
        reward,
    ];
    let mut actual = prefix.clone();
    actual.push("0x02aa".into());
    assert!(check_system_prefix(&actual, &prefix).is_ok());
    for i in 0..prefix.len() {
        let mut missing = actual.clone();
        missing.remove(i);
        assert!(check_system_prefix(&missing, &prefix).is_err());
    }
    let mut extra = actual;
    extra.push(verdicts[0].clone());
    assert!(check_system_prefix(&extra, &prefix).is_err());
    let records = decode_verdict_deposits(&c, &l1, &verdicts).unwrap();
    let mut changed = records.clone();
    changed[0].record.fuel += 1;
    let changed = verdict_deposits(&c, &l1, &changed).unwrap();
    assert!(decode_verdict_deposits(&c, &l1, &changed).is_ok());
    assert!(check_verdict_deposits(&c, &l1, &records, &changed).is_err());
}

#[test]
fn t65_51000_mutated_deposits_and_records_never_panic_or_adopt_changed_results() {
    use boole_derivation::m3::*;
    let (c, l1, verdicts, reward) = wire_input();
    let all = decode_verdict_deposits(&c, &l1, &verdicts).unwrap();
    let records = vec![all[0].clone()];
    let base = verdict_deposits(&c, &l1, &records).unwrap();
    let bases = [
        hex::decode(&base[0][2..]).unwrap(),
        hex::decode(&reward[2..]).unwrap(),
        records[0].record.encode().unwrap().to_vec(),
    ];
    let mut seed = 0x4d335f6465636f64u64;
    let mut canonical = 0;
    for i in 0..51_000usize {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let which = i % 3;
        let mut changed = bases[which].clone();
        let at = seed as usize % changed.len();
        match i % 4 {
            0 => changed[at] ^= 1 << ((seed >> 8) % 8),
            1 => {
                changed.remove(at);
            }
            2 => changed.insert(at, seed as u8),
            _ => changed.truncate(at),
        }
        let attempt = std::panic::catch_unwind(|| {
            if which == 0 {
                let tx = vec![format!("0x{}", hex::encode(&changed))];
                if let Ok(decoded) = decode_verdict_deposits(&c, &l1, &tx) {
                    assert_eq!(verdict_deposits(&c, &l1, &decoded).unwrap(), tx);
                    assert!(check_verdict_deposits(&c, &l1, &records, &tx).is_err());
                    return true;
                }
            } else if which == 1 {
                let tx = format!("0x{}", hex::encode(&changed));
                if let Ok(decoded) = decode_reward_deposit(&c, &l1, 9, &tx) {
                    assert_eq!(reward_deposit(&c, &l1, &decoded).unwrap(), tx);
                    let expected = RewardPayment {
                        reward_id: 9,
                        solver: [17; 20],
                        amount: 1_000_000_000_000_000_000_000,
                    };
                    assert!(check_reward_deposit(&c, &l1, &expected, &tx).is_err());
                    return true;
                }
            } else if let Ok(decoded) = VerdictRecord::decode(&changed) {
                assert_eq!(decoded.encode().unwrap().as_slice(), changed);
                assert_ne!(decoded, records[0].record);
                return true;
            }
            false
        });
        assert!(
            attempt.is_ok(),
            "decoder panic or wrong adoption at mutation{i}"
        );
        canonical += usize::from(attempt.unwrap());
    }
    assert!(
        canonical > 0,
        "mutations must exercise canonical-but-different semantic records too"
    );
    eprintln!(
        "51000 mutations, {canonical} canonical changes checked against the local expected result"
    );
}

#[test]
fn t40_s14_and_t45_epoch_boundary_are_exact() {
    use boole_derivation::m3::*;
    let c = config();
    let genesis = ForkRow {
        epoch: 0,
        height: 0,
        gate_rule_digest: [7; 32],
        kind: ForkKind::Genesis,
        list: vec![],
    };
    let policy = ForkRow {
        epoch: 1,
        height: 20,
        gate_rule_digest: [8; 32],
        kind: ForkKind::Policy,
        list: vec![],
    };
    let table = ForkTable {
        rows: vec![genesis.clone(), policy.clone()],
    };
    assert_eq!(table.validate(&c), Ok(()));
    assert_eq!(table.epoch_at(19).unwrap().epoch, 0);
    assert_eq!(table.epoch_at(20).unwrap().epoch, 1);
    // A job's epoch is chosen from avail+K, never from completion/runtime time.
    assert_eq!(table.job_epoch(11, 8).unwrap(), 0);
    assert_eq!(table.job_epoch(12, 8).unwrap(), 1);
    let mut cases = vec![ForkTable { rows: vec![] }];
    for which in 0..7 {
        let mut bad = table.clone();
        match which {
            0 => bad.rows[0].epoch = 1,
            1 => bad.rows[0].height = 1,
            2 => bad.rows[0].gate_rule_digest[0] ^= 1,
            3 => bad.rows[0].kind = ForkKind::Policy,
            4 => bad.rows[1].epoch = 2,
            5 => bad.rows[1].height = 0,
            _ => bad.rows[1].kind = ForkKind::Quarantine,
        };
        cases.push(bad);
    }
    for bad in cases {
        assert_eq!(bad.validate(&c), Err(14));
    }
    assert!(table.job_epoch(u64::MAX, 1).is_err());
}

#[test]
fn t41_global_queue_follows_completing_log_order_not_candidate_map_order() {
    use boole_derivation::{m2::cancel_hash, m3::*};
    use scenario::*;
    let mut c = cfg();
    let mut profile = M3Config::devnet(1);
    profile.k = 2;
    profile.maturity = 4;
    c.m3 = Some(profile);
    let (mut state, p) = opened(&c);
    let (a, ab, ap, _) = candidate(&c, &mut state, p, 7);
    let (b, bb, bp, _) = candidate(&c, &mut state, p, 8);
    let last_commit = state.height();
    until(&mut state, &c, last_commit + c.d - 1);
    run(
        &mut state,
        &c,
        vec![
            input(3, open(&ab, cancel_hash(&[8; 32])), 0),
            input(3, open(&bb, cancel_hash(&[8; 32])), 1),
        ],
    );
    let done = state.height() + 1;
    run(
        &mut state,
        &c,
        vec![
            input(4, part(a, 0, &[], &ap[0]), 9),
            input(4, part(b, 0, &[], &bp[0]), 2),
        ],
    );
    let queue = &state.m3.as_ref().unwrap().queue;
    assert_eq!(
        queue.len(),
        2,
        "each CHECKING completion needs one retained job"
    );
    assert_eq!(queue[0].key, (p, b));
    assert_eq!(queue[1].key, (p, a));
    assert_eq!((queue[0].queue_index, queue[1].queue_index), (0, 1));
    assert!(queue
        .iter()
        .all(|j| j.avail == done && j.kind == RecordKind::Verdict));
    let frozen = state.candidates[&(p, b)]
        .live()
        .unwrap()
        .open
        .as_ref()
        .unwrap();
    assert_eq!(frozen.open_pos, (done - 1, 0, 1));
    assert_eq!(frozen.done_pos, Some((done, 1, 2)));
    assert_eq!(frozen.export_digest, hash(&bp[0]));
    assert_eq!(frozen.queue_index, Some(0));
    assert_eq!(state.problems[&p].reveal_pos, (5, 0, 0));
}

#[test]
fn t44_fuel_clock_mixed_costs_cap_carry_and_checked_overflow() {
    use boole_derivation::m3::*;
    let mut p = M3Config::devnet(1);
    p.k = 2;
    p.rate = 100;
    p.cost_fixed = 10;
    let mut clock = FuelClock::default();
    assert_eq!(clock.push(&p, 2, 10, 0).unwrap(), 12);
    assert_eq!(clock.push(&p, 2, 10, 199).unwrap(), 13);
    assert_eq!(clock.push(&p, 2, 10, 0).unwrap(), 13);
    assert_eq!(clock.push(&p, 2, 10, 0).unwrap(), 14);
    assert_eq!(clock.push(&p, 2, 100, 0).unwrap(), 102);
    let before = clock.clone();
    assert_eq!(clock.push(&p, 2, u64::MAX, 0), Err(Error::Overflow));
    assert_eq!(clock, before, "overflow must leave the clock unchanged");
    let mut saturated = FuelClock {
        fuel: u128::MAX,
        ..Default::default()
    };
    assert!(saturated.push(&p, 2, 1, 0).is_err());
}

#[test]
fn t50_due_height_requires_result_then_applies_before_logs_with_a_barrier() {
    use boole_derivation::m3::*;
    use scenario::*;
    let c = m3_cfg();
    let (mut s, p, _a, b, _) = checking_with_blocker(&c);
    let done = s.height();
    run(&mut s, &c, vec![]);
    let forks = ForkTable::genesis(&c).unwrap();
    let header = block(done + 2);
    assert!(
        s.step_with_verdicts(&c, &header, &[], &[], &forks).is_err(),
        "missing result must stall at avail+K"
    );
    assert_eq!(s.height(), done + 1, "a stalled fold must not advance");
    let v = verdict(0, b, Outcome::Accept, 0, 0);
    let out = s
        .step_with_verdicts(&c, &header, &[], std::slice::from_ref(&v), &forks)
        .unwrap();
    assert_eq!(
        out.state.candidates[&(p, b)].status() as u8,
        6,
        "an earlier COMMITTED blocks settlement"
    );
    let mut expected = vec![5];
    expected.extend(0u64.to_be_bytes());
    expected.extend(v.record.encode().unwrap());
    expected.push(2);
    expected.extend(p);
    expected.extend(b);
    expected.push(6);
    assert_eq!(out.events, expected);
}

#[test]
fn t51_same_height_cancel_releases_the_barrier_after_verdict() {
    use boole_derivation::m3::*;
    use scenario::*;
    let c = m3_cfg();
    let (mut s, p, a, b, ab) = checking_with_blocker(&c);
    let done = s.height();
    run(&mut s, &c, vec![]);
    let out = s
        .step_with_verdicts(
            &c,
            &block(done + 2),
            &[input(7, cancel(p, a, &ab, [8; 32]), 0)],
            &[verdict(0, b, Outcome::Accept, 0, 0)],
            &ForkTable::genesis(&c).unwrap(),
        )
        .unwrap();
    assert_eq!(out.codes, [0x84]);
    assert_eq!(out.state.candidates[&(p, b)].status() as u8, 7);
    assert_eq!(out.state.problems[&p].winner, Some(b));
    assert_eq!(out.state.problems[&p].closed, Some(done + 2));
    assert_eq!(
        out.state.slots[0].scheduled,
        Some(done + 2 + c.slot_table[0].announce_gap)
    );
    assert!(out.events.windows(7).any(|x| x == [3, 0, 0, 0, 0, 1, 0xc5]));
    assert!(
        out.events.windows(1).any(|x| x == [6]),
        "settlement must create a deferred reward"
    );
    assert!(
        out.events.windows(1).any(|x| x == [7]),
        "settlement must assign an admission"
    );
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/devnet-m3/system-v1.3.json")).unwrap();
    assert_eq!(hex::encode(p), golden["settlement"]["problem_id"]);
    assert_eq!(hex::encode(b), golden["settlement"]["commit_hash"]);
    assert_eq!(hex::encode(&out.events), golden["settlement"]["event_hex"]);
    assert_eq!(
        hex::encode(hash(&out.events)),
        golden["settlement"]["event_digest"]
    );
}

#[test]
fn t56_deferred_reward_exact_vesting_payee_mint_off_and_overflow() {
    use boole_derivation::m3::*;
    use scenario::*;
    for mint in [true, false] {
        let mut c = m3_cfg();
        let profile = c.m3.as_mut().unwrap();
        profile.mint_enabled = mint;
        profile.q_reward = 1;
        let (mut s, p, b) = settled(&c);
        let reward = s.m3.as_ref().unwrap().rewards[&0].clone();
        assert_eq!(reward.key, (p, b));
        assert_eq!(reward.solver, [6; 20]);
        assert_ne!(reward.solver, s.candidates[&(p, b)].live().unwrap().poster);
        assert_eq!(reward.status, RewardStatus::Locked);
        assert_eq!(reward.vest_h, s.height() + 4);
        let mut second = reward.clone();
        second.id = 1;
        s.m3.as_mut().unwrap().rewards.insert(1, second);
        until(&mut s, &c, reward.vest_h - 1);
        assert_eq!(
            s.m3.as_ref().unwrap().rewards[&0].status,
            RewardStatus::Locked
        );
        let out = s
            .step_with_verdicts(
                &c,
                &block(reward.vest_h),
                &[],
                &[],
                &ForkTable::genesis(&c).unwrap(),
            )
            .unwrap();
        assert_eq!(
            out.state.m3.as_ref().unwrap().rewards[&0].status,
            RewardStatus::Vested
        );
        assert_eq!(
            out.state.m3.as_ref().unwrap().rewards[&1].status,
            RewardStatus::Locked
        );
        assert_eq!(out.payments.len(), usize::from(mint));
        if mint {
            assert_eq!(out.payments[0].solver, [6; 20]);
            assert_eq!(out.payments[0].amount, reward.amount);
        }
        let next = out
            .state
            .step_with_verdicts(
                &c,
                &block(reward.vest_h + 1),
                &[],
                &[],
                &ForkTable::genesis(&c).unwrap(),
            )
            .unwrap();
        assert_eq!(
            next.state.m3.as_ref().unwrap().rewards[&1].status,
            RewardStatus::Vested
        );
    }
}

#[test]
fn t53_problem_void_closes_once_and_later_completions_never_queue() {
    use boole_derivation::{m2::cancel_hash, m3::*};
    use scenario::*;
    let c = m3_cfg();
    let (mut base, p, a, b, ab) = checking_with_blocker(&c);
    run(&mut base, &c, vec![]);
    let h = base.height() + 1;
    let forks = ForkTable::genesis(&c).unwrap();
    for reason in [0x0801, 0x0802, 0x0803, 0x0804] {
        let out = base
            .step_with_verdicts(
                &c,
                &block(h),
                &[],
                &[verdict(0, b, Outcome::Void, reason, 0)],
                &forks,
            )
            .unwrap();
        assert_eq!(out.state.candidates[&(p, b)].status() as u8, 9);
        assert_eq!(
            out.state.problems[&p].voided,
            reason != 0x0802,
            "problem disposition for {reason:#x}"
        );
        if reason == 0x0802 {
            assert!(out.state.problems[&p].closed.is_none());
            continue;
        }
        assert_eq!(out.state.problems[&p].closed, Some(h));
        assert!(out.state.m3.as_ref().unwrap().rewards.is_empty());
        let mut s = out.state;
        assert_eq!(
            run(
                &mut s,
                &c,
                vec![input(3, open(&ab, cancel_hash(&[8; 32])), 0)]
            ),
            [0x81]
        );
        assert_eq!(
            run(&mut s, &c, vec![input(4, part(a, 0, &[], &[42; 7]), 0)]),
            [0x83]
        );
        assert_eq!(s.candidates[&(p, a)].status() as u8, 9);
        assert_eq!(
            s.m3.as_ref().unwrap().queue.len(),
            1,
            "post-void completion must not enqueue"
        );
    }
}

#[test]
fn t54_root_v3_commits_post_verdict_events_and_refolds_identically() {
    use boole_derivation::m3::*;
    use scenario::*;
    let c = m3_cfg();
    let (mut s, _p, _a, b, _) = checking_with_blocker(&c);
    run(&mut s, &c, vec![]);
    let h = s.height() + 1;
    let header = block(h);
    let v = verdict(0, b, Outcome::Reject, 0x0306, 11);
    let first = s
        .step_with_verdicts(
            &c,
            &header,
            &[],
            std::slice::from_ref(&v),
            &ForkTable::genesis(&c).unwrap(),
        )
        .unwrap();
    let expected = hash(
        &[
            b"BOOLE-M2/ROOT/v3".as_slice(),
            &s.root,
            &h.to_be_bytes(),
            &hex::decode(&header.hash[2..]).unwrap(),
            &hash(&first.events),
        ]
        .concat(),
    );
    assert_eq!(first.state.root, expected, "M3 must use the ROOT/v3 domain");
    let again = s
        .step_with_verdicts(&c, &header, &[], &[v], &ForkTable::genesis(&c).unwrap())
        .unwrap();
    assert_eq!(first.events, again.events);
    assert_eq!(first.state.root, again.state.root);
    let (mut replay, _, _, replay_b, _) = checking_with_blocker(&c);
    run(&mut replay, &c, vec![]);
    let rebuilt = replay
        .step_with_verdicts(
            &c,
            &header,
            &[],
            &[verdict(0, replay_b, Outcome::Reject, 0x0306, 11)],
            &ForkTable::genesis(&c).unwrap(),
        )
        .unwrap();
    assert_eq!(rebuilt.events, first.events);
    assert_eq!(
        rebuilt.state.root, first.state.root,
        "full replay from genesis must match the incremental root"
    );
    let deposit = boole_derivation::system::m2_root_deposit(&header, first.state.root).unwrap();
    assert_eq!(
        boole_derivation::system::read_m2_root(&deposit, header.number, &header.hash).unwrap(),
        first.state.root
    );
}

#[test]
fn t47_t56_attributes_prefix_and_actual_counts_reserve_before_f6() {
    use boole_derivation::{attributes::derive_with_m3, m3::*};
    use scenario::*;
    let c = m3_cfg();
    let (mut s, _p, _a, b, _) = checking_with_blocker(&c);
    run(&mut s, &c, vec![]);
    let header = block(s.height() + 1);
    let parent = parent(s.height());
    let v = verdict(0, b, Outcome::Reject, 0x0101, 0);
    let forks = ForkTable::genesis(&c).unwrap();
    let (exact, next) = derive_with_m3(
        &s,
        &c,
        &header,
        &user_logs(&header, &[27_000_000]),
        &parent,
        std::slice::from_ref(&v),
        &forks,
    )
    .unwrap();
    assert!(exact.decisions[0].forward);
    assert_eq!(exact.attributes.transactions.len(), 4);
    assert_eq!(
        exact.attributes.transactions[1],
        boole_derivation::system::m2_root_deposit(&header, next.root).unwrap()
    );
    assert_eq!(
        exact.attributes.transactions[2],
        verdict_deposits(&c, &header, std::slice::from_ref(&v)).unwrap()[0]
    );
    let (over, _) = derive_with_m3(
        &s,
        &c,
        &header,
        &user_logs(&header, &[27_000_001, 21_000, 26_979_000]),
        &parent,
        &[v],
        &forks,
    )
    .unwrap();
    assert!(!over.decisions[0].forward);
    assert!(over.decisions[1].forward && over.decisions[2].forward);
    let (mut paid, _, _) = settled(&c);
    let vest_h = paid.m3.as_ref().unwrap().rewards[&0].vest_h;
    until(&mut paid, &c, vest_h - 1);
    let header = block(vest_h);
    let parent = scenario::parent(vest_h - 1);
    let (attrs, _) = derive_with_m3(
        &paid,
        &c,
        &header,
        &user_logs(&header, &[27_900_000]),
        &parent,
        &[],
        &forks,
    )
    .unwrap();
    assert!(attrs.decisions[0].forward);
    assert_eq!(attrs.attributes.transactions.len(), 4);
    let reward = decode_reward_deposit(&c, &header, 0, &attrs.attributes.transactions[2]).unwrap();
    assert_eq!(reward.solver, [6; 20]);
    let (attrs, _) = derive_with_m3(
        &paid,
        &c,
        &header,
        &user_logs(&header, &[27_900_001]),
        &parent,
        &[],
        &forks,
    )
    .unwrap();
    assert!(!attrs.decisions[0].forward);
}

#[test]
fn t50_final_m3_statuses_cannot_be_reopened_completed_again_or_cancelled() {
    use boole_derivation::{
        m2::{cancel_hash, Candidate, Status},
        m3::*,
    };
    use scenario::*;
    let c = m3_cfg();
    let (mut base, p, _a, b, _) = checking_with_blocker(&c);
    run(&mut base, &c, vec![]);
    let mut s = base
        .step_with_verdicts(
            &c,
            &block(base.height() + 1),
            &[],
            &[verdict(0, b, Outcome::Accept, 0, 0)],
            &ForkTable::genesis(&c).unwrap(),
        )
        .unwrap()
        .state;
    let (bb, parts, _) = body(&c, p, [6; 20], [7; 32], &[42; 8], &[]);
    for target in [
        Status::Passed,
        Status::Accepted,
        Status::Rejected,
        Status::Void,
        Status::Stale,
        Status::Superseded,
    ] {
        if let Candidate::Live(l) = s.candidates.get_mut(&(p, b)).unwrap() {
            l.status = target;
        }
        let out = s
            .step_with_verdicts(
                &c,
                &block(s.height() + 1),
                &[
                    input(3, open(&bb, cancel_hash(&[8; 32])), 0),
                    input(4, part(b, 0, &[], &parts[0]), 1),
                    input(7, cancel(p, b, &bb, [8; 32]), 2),
                ],
                &[],
                &ForkTable::genesis(&c).unwrap(),
            )
            .unwrap();
        assert_eq!(out.codes, [10, 10, 9], "immutable final status {target:?}");
    }
}

#[test]
fn t57_stale_cancels_only_locked_retires_and_reposes_frozen_problem() {
    use boole_derivation::{m2::cancel_hash, m3::*};
    use scenario::*;
    for after_vest in [false, true] {
        let c = m3_cfg();
        let (mut s, p, b) = settled(&c);
        let original = s.problems[&p].clone();
        let vest_h = s.m3.as_ref().unwrap().rewards[&0].vest_h;
        if after_vest {
            until(&mut s, &c, vest_h);
        }
        let v = recheck_fixture(&mut s, (p, b), 0);
        let h = s.height() + 2;
        run(&mut s, &c, vec![]);
        assert!(matches!(
            s.step_with_verdicts(&c, &block(h), &[], &[], &ForkTable::genesis(&c).unwrap()),
            Err(boole_derivation::m2::FoldError::M3(Error::Pending { .. }))
        ));
        assert_eq!(
            s.candidates[&(p, b)].status() as u8,
            7,
            "DEFER never makes a recheck target STALE"
        );
        let out = s
            .step_with_verdicts(&c, &block(h), &[], &[v], &ForkTable::genesis(&c).unwrap())
            .unwrap();
        assert_eq!(out.state.candidates[&(p, b)].status() as u8, 10);
        let m = out.state.m3.as_ref().unwrap();
        assert_eq!(
            m.rewards[&0].status,
            if after_vest {
                RewardStatus::Vested
            } else {
                RewardStatus::Cancelled
            }
        );
        assert!(m.admissions[&1].retired);
        assert!(m.pending_rechecks.is_empty());
        assert!(out.events.windows(7).any(|x| x == [3, 0, 0, 0, 0, 1, 0xc7]));
        let mut s = out.state;
        until(&mut s, &c, h + c.slot_table[0].announce_gap + 2);
        let new = s.problems.values().find(|p| p.epoch == 1).unwrap();
        assert_ne!(new.id, p);
        assert_eq!(new.reveal_pos, original.reveal_pos);
        assert_eq!(
            (new.pool_seq, new.leaf_index, new.instance_seed),
            (
                original.pool_seq,
                original.leaf_index,
                original.instance_seed
            )
        );
        assert!(new.winner.is_none() && !new.voided && new.closed.is_none());
        let new_id = new.id;
        assert_eq!(run(&mut s, &c, vec![input(2, commit(p, [9; 32]), 0)]), [5]);
        let (ch, body, parts, _) = candidate(&c, &mut s, new_id, 9);
        let hc = s.height();
        until(&mut s, &c, hc + c.d - 1);
        run(
            &mut s,
            &c,
            vec![input(3, open(&body, cancel_hash(&[8; 32])), 0)],
        );
        run(&mut s, &c, vec![input(4, part(ch, 0, &[], &parts[0]), 0)]);
        let index = s.m3.as_ref().unwrap().queue.last().unwrap().queue_index;
        let due = s.height() + 2;
        run(&mut s, &c, vec![]);
        let next = s
            .step_with_verdicts(
                &c,
                &block(due),
                &[],
                &[verdict(index, ch, Outcome::Accept, 0, 0)],
                &ForkTable::genesis(&c).unwrap(),
            )
            .unwrap();
        let admissions = &next.state.m3.as_ref().unwrap().admissions;
        assert!(admissions[&1].retired);
        assert_eq!(admissions[&2].key, (new_id, ch));
        assert_eq!(next.state.problems[&new_id].winner, Some(ch));
    }
}

#[test]
fn t58_kernel_fault_reversal_after_winner_compensates_once_without_reopening() {
    use boole_derivation::m3::*;
    use scenario::*;
    let c = m3_cfg();
    let (mut s, p, a, b) = kernel_fault_and_winner(&c);
    assert_eq!(s.problems[&p].winner, Some(b));
    assert_eq!(s.candidates[&(p, a)].status() as u8, 8);
    let closed = s.problems[&p].closed;
    let scheduled = s.slots[0].scheduled;
    let mut v = recheck_fixture(&mut s, (p, a), 0);
    v.record.outcome = Outcome::Accept;
    v.record.reason = 0;
    let h = s.height() + 2;
    run(&mut s, &c, vec![]);
    let out = s
        .step_with_verdicts(&c, &block(h), &[], &[v], &ForkTable::genesis(&c).unwrap())
        .unwrap();
    assert_eq!(out.state.candidates[&(p, a)].status() as u8, 7);
    assert_eq!(out.state.problems[&p].winner, Some(b));
    assert_eq!(out.state.problems[&p].closed, closed);
    assert_eq!(out.state.slots[0].scheduled, scheduled);
    let m = out.state.m3.as_ref().unwrap();
    assert_eq!(m.rewards.len(), 2);
    assert_eq!(m.admissions.len(), 2);
    assert!(!m.rewards[&0].compensation);
    assert!(m.rewards[&1].compensation);
    assert_eq!(m.rewards[&1].key, (p, a));
    assert_eq!(m.rewards[&1].vest_h, h + 4);
    assert_eq!(m.admissions[&1].key, (p, b));
    assert_eq!(m.admissions[&2].key, (p, a));
    let mut s = out.state;
    let mut v = recheck_fixture(&mut s, (p, a), 0);
    v.record.outcome = Outcome::Accept;
    v.record.reason = 0;
    let h = s.height() + 2;
    run(&mut s, &c, vec![]);
    let out = s
        .step_with_verdicts(&c, &block(h), &[], &[v], &ForkTable::genesis(&c).unwrap())
        .unwrap();
    assert_eq!(
        out.state.m3.as_ref().unwrap().rewards.len(),
        2,
        "ACCEPT recheck cannot mint a second compensation"
    );
}

#[test]
fn t40_three_staged_profiles_and_empty_roots_match_python_config_v2_golden() {
    use boole_derivation::{
        m2::{Config, State},
        m3::*,
    };
    use scenario::*;
    let v: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/devnet-m3/system-v1.3.json")).unwrap();
    for (index, mut c) in [
        Config::devnet(),
        Config::lean_testnet(720927),
        Config::production(720928),
    ]
    .into_iter()
    .enumerate()
    {
        c.m3 = Some(if index == 0 {
            M3Config::devnet(c.slot_table.len())
        } else {
            M3Config::lean_testnet(c.slot_table.len())
        });
        let row = &v["profiles"][index];
        assert_eq!(
            hex::encode(serde_json::to_vec(&c).unwrap()),
            row["canonical_json_hex"].as_str().expect("profile golden")
        );
        assert_eq!(
            hex::encode(c.digest()),
            row["config_digest"].as_str().unwrap()
        );
        let s = State::new(&c, if index == 0 { 30_000_000 } else { 60_000_000 }).unwrap();
        let out = s
            .step_with_verdicts(&c, &block(1), &[], &[], &ForkTable::genesis(&c).unwrap())
            .unwrap();
        assert!(out.events.is_empty());
        assert_eq!(
            hex::encode(out.state.root),
            row["empty_root_1"].as_str().unwrap()
        );
    }
}

#[test]
fn t59_kernel_fix_plan_uses_original_queue_order_and_exact_recheck_set() {
    use boole_derivation::{
        m2::{Candidate, Status},
        m3::*,
    };
    use scenario::*;
    let c = m3_cfg();
    let (mut s, p, a, b) = kernel_fault_and_winner(&c);
    let h = s.height() + 1;
    let mut table = ForkTable::genesis(&c).unwrap();
    table.rows.push(ForkRow {
        epoch: 1,
        height: h,
        gate_rule_digest: hash(b"fixed gate"),
        kind: ForkKind::KernelFix,
        list: vec![],
    });
    assert_eq!(table.recheck_set(&s, h).unwrap(), vec![(p, b), (p, a)]);
    if let Candidate::Live(l) = s.candidates.get_mut(&(p, a)).unwrap() {
        l.status = Status::Passed;
    }
    assert_eq!(table.recheck_set(&s, h).unwrap(), vec![(p, b), (p, a)]);
    s.m3.as_mut()
        .unwrap()
        .results
        .get_mut(&(p, b))
        .unwrap()
        .gate_epoch = 1;
    assert_eq!(table.recheck_set(&s, h).unwrap(), vec![(p, a)]);
    for status in [
        Status::Committed,
        Status::Open,
        Status::Checking,
        Status::Expired,
        Status::Cancelled,
        Status::Void,
        Status::Stale,
        Status::Superseded,
    ] {
        if let Candidate::Live(l) = s.candidates.get_mut(&(p, a)).unwrap() {
            l.status = status;
        }
        assert!(table.recheck_set(&s, h).unwrap().is_empty());
    }
    table.rows[1].kind = ForkKind::Policy;
    assert!(table.recheck_set(&s, h).unwrap().is_empty());
    let out = s
        .step_with_verdicts(&c, &block(h), &[], &[], &table)
        .unwrap();
    assert_eq!(
        &out.events[1..70],
        &table.rows[1].activation_record().unwrap()
    );
    assert_eq!(
        out.state.m3.as_ref().unwrap().queue.len(),
        s.m3.as_ref().unwrap().queue.len()
    );
}

#[test]
fn t41_t51_t62_cross_slot_queue_and_problem_ordered_rewards_admissions() {
    use boole_derivation::{
        m2::{body_digest, cancel_hash, commit_hash, State},
        m3::*,
    };
    use scenario::*;
    let mut c = m3_cfg();
    c.slot_table.push(c.slot_table[0].clone());
    let p = c.m3.as_mut().unwrap();
    p.fuel_slot.push(10_000);
    p.reward = vec![100, 200];
    let mut s = State::new(&c, 30_000_000).unwrap();
    let roots = [
        pool_leaf(c.chain_id, 2, &[], b"a", &[1; 32]),
        pool_leaf(c.chain_id, 2, &[], b"b", &[2; 32]),
    ];
    run(
        &mut s,
        &c,
        vec![
            input(8, pool_at(&c, 0, roots[0], 1, 3, 1), 0),
            input(8, pool_at(&c, 1, roots[1], 1, 3, 2), 1),
        ],
    );
    until(&mut s, &c, 4);
    run(
        &mut s,
        &c,
        vec![
            input(9, problem_body(0, 1, 0, &[], [1; 32], &[], b"a"), 0),
            input(9, problem_body(1, 1, 0, &[], [2; 32], &[], b"b"), 1),
        ],
    );
    let mut proofs = Vec::new();
    for pid in s.problems.keys().copied() {
        let (b, parts, _) = body(&c, pid, [6; 20], [7; 32], &[42; 7], &[]);
        let ch = commit_hash(c.chain_id, pid, cancel_hash(&[8; 32]), body_digest(&b));
        proofs.push((pid, ch, b, parts));
    }
    run(
        &mut s,
        &c,
        proofs
            .iter()
            .enumerate()
            .map(|(i, (p, ch, _, _))| input(2, commit(*p, *ch), i as u64))
            .collect(),
    );
    until(&mut s, &c, 14);
    run(
        &mut s,
        &c,
        proofs
            .iter()
            .enumerate()
            .map(|(i, (_, _, b, _))| input(3, open(b, cancel_hash(&[8; 32])), i as u64))
            .collect(),
    );
    run(
        &mut s,
        &c,
        proofs
            .iter()
            .rev()
            .enumerate()
            .map(|(i, (_, ch, _, parts))| input(4, part(*ch, 0, &[], &parts[0]), i as u64))
            .collect(),
    );
    assert_eq!(
        s.m3.as_ref()
            .unwrap()
            .queue
            .iter()
            .map(|j| j.key.0)
            .collect::<Vec<_>>(),
        vec![proofs[1].0, proofs[0].0]
    );
    run(&mut s, &c, vec![]);
    let records = vec![
        verdict(0, proofs[1].1, Outcome::Accept, 0, 0),
        verdict(1, proofs[0].1, Outcome::Accept, 0, 0),
    ];
    let out = s
        .step_with_verdicts(
            &c,
            &block(18),
            &[],
            &records,
            &ForkTable::genesis(&c).unwrap(),
        )
        .unwrap();
    let m = out.state.m3.as_ref().unwrap();
    for (i, (p, ch, _, _)) in proofs.iter().enumerate() {
        assert_eq!(out.state.problems[p].winner, Some(*ch));
        assert_eq!(m.rewards[&(i as u64)].key, (*p, *ch));
        assert_eq!(m.admissions[&(i as u64 + 1)].key, (*p, *ch));
        assert_eq!(
            m.rewards[&(i as u64)].amount,
            c.m3.as_ref().unwrap().reward[out.state.problems[p].slot as usize]
        );
    }
    let mut retained = out.state;
    until(&mut retained, &c, 84);
    assert_eq!(retained.m3.as_ref().unwrap().queue.len(), 2);
    assert_eq!(retained.m3.as_ref().unwrap().admissions.len(), 2);
    for (p, ch, _, _) in proofs {
        assert!(retained.candidates[&(p, ch)].live().unwrap().open.is_some());
    }
}

#[test]
fn t52_late_settlement_of_old_problem_leaves_new_round_active() {
    use boole_derivation::m3::*;
    use scenario::*;
    let mut c = m3_cfg();
    c.slot_table[0].solve_deadline = 12;
    let (mut s, p, a, b, ab) = checking_with_blocker(&c);
    run(&mut s, &c, vec![input(7, cancel(p, a, &ab, [8; 32]), 0)]);
    assert_eq!(s.problems[&p].closed, Some(18));
    let v = verdict(0, b, Outcome::Accept, 0, 10_000);
    let forks = ForkTable::genesis(&c).unwrap();
    for h in 19..28 {
        let items = match h {
            19 => vec![input(
                8,
                pool(
                    &c,
                    pool_leaf(c.chain_id, 2, &[], b"next", &[10; 32]),
                    1,
                    22,
                    2,
                ),
                0,
            )],
            24 => vec![input(
                9,
                problem_body(0, 2, 0, &[], [10; 32], &[], b"next"),
                0,
            )],
            _ => vec![],
        };
        let out = s
            .step_with_verdicts(&c, &block(h), &items, std::slice::from_ref(&v), &forks)
            .unwrap();
        assert!(out.verdicts.is_empty());
        s = out.state;
    }
    assert_eq!(s.slots[0].active, Some((0, 2)));
    let out = s
        .step_with_verdicts(&c, &block(28), &[], &[v], &forks)
        .unwrap();
    assert_eq!(out.state.problems[&p].winner, Some(b));
    assert_eq!(out.state.problems[&p].closed, Some(18));
    assert_eq!(out.state.slots[0].active, Some((0, 2)));
    assert!(out.state.slots[0].scheduled.is_none());
    assert_eq!(
        out.state.rounds[&(0, 2)].status,
        boole_derivation::m2::RoundStatus::Open
    );
}

#[test]
fn t55_real_service_vectors_use_m3_verdict_application_and_barriers() {
    use boole_derivation::{
        m2::{body_digest, cancel_hash, commit_hash, Status},
        m3::*,
    };
    use scenario::*;
    use std::collections::BTreeMap;
    let vectors: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/service-parity-v1.json"
    ))
    .unwrap();
    assert_eq!(
        vectors["provenance"]["source_sha256"],
        hex::encode(hash(include_bytes!(
            "../../../scripts/zk_registry/service.py"
        )))
    );
    for case in vectors["cases"].as_array().unwrap() {
        // The registry decides synchronously. Reserve K chain heights after
        // each registry action and compare at the end of that interval. The
        // inclusive M2 window is scaled to preserve the registry's boundary.
        let mut c = m3_cfg();
        let k = c.m3.as_ref().unwrap().k;
        let scale = k + 1;
        c.d = vectors["mapping"]["d"].as_u64().unwrap() * scale;
        c.w = vectors["mapping"]["w"].as_u64().unwrap() * scale + k;
        c.slot_table[0].solve_deadline = 1024;
        c.m3.as_mut().unwrap().mint_enabled = false;
        let (mut s, p) = opened(&c);
        let forks = ForkTable::genesis(&c).unwrap();
        let mut proofs = BTreeMap::new();
        let mut sequences = BTreeMap::new();
        let mut outcomes = BTreeMap::new();
        for row in case["rows"].as_array().unwrap() {
            let logical = row["height"].as_u64().unwrap();
            if logical <= 5 {
                continue;
            }
            let start = 5 + (logical - 5) * scale;
            until(&mut s, &c, start - 1);
            let mut items = Vec::new();
            for action in row["actions"].as_array().unwrap() {
                let seq = action["sequence"].as_u64().unwrap();
                let pos = items.len() as u64;
                if action["op"] == "commit" {
                    let (b, parts, _) =
                        body(&c, p, [6; 20], hash(&seq.to_be_bytes()), &[seq as u8], &[]);
                    let ch = commit_hash(c.chain_id, p, cancel_hash(&[8; 32]), body_digest(&b));
                    items.push(input(2, commit(p, ch), pos));
                    sequences.insert(ch, seq);
                    proofs.insert(seq, (ch, b, parts[0].clone()));
                } else {
                    let (ch, b, data) = &proofs[&seq];
                    let mut opening = b.clone();
                    if action["correct_binding"] == false {
                        opening[53] ^= 1;
                    }
                    items.push(input(3, open(&opening, cancel_hash(&[8; 32])), pos));
                    if action["matched"] == true {
                        items.push(input(4, part(*ch, 0, &[], data), pos + 1));
                        outcomes.insert(*ch, action["verdict"] == "PASS");
                    }
                }
            }
            run(&mut s, &c, items);
            for h in start + 1..=start + k {
                let records: Vec<_> = if h == start + k {
                    let m = s.m3.as_ref().unwrap();
                    m.queue[m.next_applied..]
                        .iter()
                        .map(|job| {
                            let pass = outcomes[&job.key.1];
                            verdict(
                                job.queue_index,
                                job.key.1,
                                if pass {
                                    Outcome::Accept
                                } else {
                                    Outcome::Reject
                                },
                                if pass { 0 } else { 0x0301 },
                                0,
                            )
                        })
                        .collect()
                } else {
                    vec![]
                };
                s = s
                    .step_with_verdicts(&c, &block(h), &[], &records, &forks)
                    .unwrap()
                    .state;
            }
            let actual: BTreeMap<String, &str> = proofs
                .iter()
                .map(|(seq, (ch, _, _))| {
                    let status = match s.candidates[&(p, *ch)].status() {
                        Status::Committed | Status::Open => "COMMITTED",
                        Status::Expired => "EXPIRED",
                        Status::Passed => "VALID-REVEAL",
                        Status::Accepted => "ACCEPTED",
                        Status::Rejected => "REJECTED",
                        Status::Superseded => "DUPLICATE",
                        other => panic!("unmapped parity status {other:?}"),
                    };
                    (seq.to_string(), status)
                })
                .collect();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                row["expected"]["statuses"],
                "{} registry height {logical}",
                case["name"]
            );
            let winner = s.problems[&p].winner.map(|ch| sequences[&ch]);
            assert_eq!(
                serde_json::to_value(winner).unwrap(),
                row["expected"]["winner"],
                "{} registry height {logical}",
                case["name"]
            );
        }
    }
}

#[test]
fn t50_complete_verdict_and_recheck_target_matrix_is_atomic() {
    use boole_derivation::{
        m2::{Candidate, Status},
        m3::*,
    };
    use scenario::*;
    let c = m3_cfg();
    let (mut base, p, _a, b, _) = checking_with_blocker(&c);
    run(&mut base, &c, vec![]);
    let h = base.height() + 1;
    let forks = ForkTable::genesis(&c).unwrap();
    let statuses = [
        Status::Committed,
        Status::Open,
        Status::Checking,
        Status::Expired,
        Status::Cancelled,
        Status::Passed,
        Status::Accepted,
        Status::Rejected,
        Status::Void,
        Status::Stale,
        Status::Superseded,
    ];
    for kind in [RecordKind::Verdict, RecordKind::Recheck] {
        for old in statuses {
            for outcome in [Outcome::Accept, Outcome::Reject, Outcome::Void] {
                let mut s = base.clone();
                if let Candidate::Live(l) = s.candidates.get_mut(&(p, b)).unwrap() {
                    l.status = old;
                }
                let m = s.m3.as_mut().unwrap();
                m.queue[0].kind = kind;
                if kind == RecordKind::Recheck {
                    m.pending_rechecks.insert((p, b));
                }
                m.results.insert(
                    (p, b),
                    verdict(0, b, Outcome::Reject, KERNEL_FAULT, 0).record,
                );
                let mut v = verdict(
                    0,
                    b,
                    outcome,
                    match outcome {
                        Outcome::Accept => 0,
                        Outcome::Reject => 0x0301,
                        Outcome::Void => 0x0802,
                    },
                    0,
                );
                v.record.kind = kind;
                let expected = match (kind, old, outcome) {
                    (RecordKind::Verdict, Status::Checking, Outcome::Accept) => {
                        Some(Status::Passed)
                    }
                    (RecordKind::Verdict, Status::Checking, Outcome::Reject) => {
                        Some(Status::Rejected)
                    }
                    (RecordKind::Verdict, Status::Checking, Outcome::Void) => Some(Status::Void),
                    (RecordKind::Verdict, Status::Void | Status::Superseded, _) => Some(old),
                    (RecordKind::Recheck, Status::Accepted | Status::Passed, Outcome::Accept) => {
                        Some(old)
                    }
                    (RecordKind::Recheck, Status::Accepted, Outcome::Reject) => Some(Status::Stale),
                    (RecordKind::Recheck, Status::Passed | Status::Rejected, Outcome::Reject) => {
                        Some(Status::Rejected)
                    }
                    (RecordKind::Recheck, Status::Rejected, Outcome::Accept) => {
                        Some(Status::Passed)
                    }
                    _ => None,
                };
                let result = s.step_with_verdicts(&c, &block(h), &[], &[v], &forks);
                if let Some(expected) = expected {
                    let out = result.unwrap();
                    assert_eq!(
                        out.state.candidates[&(p, b)].status(),
                        expected,
                        "{kind:?} {old:?} {outcome:?}"
                    );
                    assert_eq!(out.events[0], 5);
                    assert_eq!(out.state.m3.as_ref().unwrap().next_applied, 1);
                } else {
                    assert!(result.is_err(), "{kind:?} {old:?} {outcome:?}");
                }
                assert_eq!(s.root, base.root);
                assert_eq!(s.height(), base.height());
                assert_eq!(s.candidates[&(p, b)].status(), old);
            }
        }
    }
    let mut s = base.clone();
    if let Candidate::Live(l) = s.candidates.get_mut(&(p, b)).unwrap() {
        l.status = Status::Rejected;
    }
    let m = s.m3.as_mut().unwrap();
    m.queue[0].kind = RecordKind::Recheck;
    m.results
        .insert((p, b), verdict(0, b, Outcome::Reject, 0x0101, 0).record);
    let mut v = verdict(0, b, Outcome::Accept, 0, 0);
    v.record.kind = RecordKind::Recheck;
    assert!(
        s.step_with_verdicts(&c, &block(h), &[], &[v], &forks)
            .is_err(),
        "only kernel-fault rejects may be reversed"
    );
}

#[test]
fn t44_fold_applies_only_cap_prefix_and_rejects_wrong_job_inputs() {
    use boole_derivation::{
        m2::{Candidate, Status},
        m3::*,
    };
    use scenario::*;
    let mut c = m3_cfg();
    c.q_max = 1;
    c.v_per_deposit = 2;
    c.c_commit = 2;
    let (mut s, p, _a, b, _) = checking_with_blocker(&c);
    let original = s.candidates[&(p, b)].clone();
    s.candidates.remove(&(p, b));
    s.m3.as_mut().unwrap().queue.clear();
    let mut records = Vec::new();
    for i in 0..5u64 {
        let ch = hash(&i.to_be_bytes());
        let mut r = original.clone();
        if let Candidate::Live(l) = &mut r {
            l.status = Status::Checking;
            l.open.as_mut().unwrap().queue_index = Some(i);
        }
        s.candidates.insert((p, ch), r);
        s.m3.as_mut().unwrap().queue.push(Job {
            queue_index: i,
            key: (p, ch),
            kind: RecordKind::Verdict,
            avail: 17,
            epoch_override: None,
        });
        records.push(verdict(i, ch, Outcome::Reject, 0x0101, 0));
    }
    run(&mut s, &c, vec![]);
    let forks = ForkTable::genesis(&c).unwrap();
    for which in 0..5 {
        let mut bad = records.clone();
        match which {
            0 => bad[0].queue_index += 1,
            1 => bad[0].record.commit_hash[0] ^= 1,
            2 => bad[0].record.kind = RecordKind::Recheck,
            3 => bad[0].record.gate_epoch = 1,
            _ => bad[0].record.fuel = 10_001,
        };
        assert!(s
            .step_with_verdicts(&c, &block(19), &[], &bad, &forks)
            .is_err());
    }
    for (h, start, end) in [(19, 0, 2), (20, 2, 4), (21, 4, 5)] {
        let out = s
            .step_with_verdicts(&c, &block(h), &[], &records[start..], &forks)
            .unwrap();
        assert_eq!(out.verdicts, records[start..end]);
        assert_eq!(out.state.m3.as_ref().unwrap().next_applied, end);
        assert_eq!(
            verdict_deposits(&c, &block(h), &out.verdicts)
                .unwrap()
                .len(),
            1
        );
        s = out.state;
    }
    assert_eq!(s.m3.as_ref().unwrap().queue.len(), 5);
}

#[test]
fn t59_retroactive_fork_row_is_refused_but_future_rows_are_allowed() {
    use boole_derivation::m3::*;
    use scenario::*;
    let c = m3_cfg();
    let (s, _, _) = settled(&c);
    let h = s.height() + 1;
    let mut table = ForkTable::genesis(&c).unwrap();
    table.rows.push(ForkRow {
        epoch: 1,
        height: 10,
        gate_rule_digest: hash(b"retroactive policy"),
        kind: ForkKind::Policy,
        list: vec![],
    });
    assert!(
        s.step_with_verdicts(&c, &block(h), &[], &[], &table)
            .is_err(),
        "a past row cannot be silently added"
    );
    table.rows[1].height = 100;
    assert!(s
        .step_with_verdicts(&c, &block(h), &[], &[], &table)
        .is_ok());
}

#[test]
fn t59_passed_candidate_waits_for_recheck_even_after_barrier_cancel() {
    use boole_derivation::m3::*;
    use scenario::*;
    let c = m3_cfg();
    let (mut s, p, a, b, ab) = checking_with_blocker(&c);
    run(&mut s, &c, vec![]);
    let forks = ForkTable::genesis(&c).unwrap();
    s = s
        .step_with_verdicts(
            &c,
            &block(19),
            &[],
            &[verdict(0, b, Outcome::Accept, 0, 0)],
            &forks,
        )
        .unwrap()
        .state;
    let mut forks = forks;
    forks.rows.push(ForkRow {
        epoch: 1,
        height: 20,
        gate_rule_digest: hash(b"passed recheck gate"),
        kind: ForkKind::KernelFix,
        list: vec![],
    });
    let mut v = verdict(1, b, Outcome::Accept, 0, 0);
    v.record.kind = RecordKind::Recheck;
    v.record.gate_epoch = 1;
    s = s
        .step_with_verdicts(
            &c,
            &block(20),
            &[input(7, cancel(p, a, &ab, [8; 32]), 0)],
            &[],
            &forks,
        )
        .unwrap()
        .state;
    assert_eq!(s.candidates[&(p, b)].status() as u8, 6);
    assert!(s.problems[&p].winner.is_none());
    assert!(s.m3.as_ref().unwrap().rewards.is_empty());
    s = s
        .step_with_verdicts(&c, &block(21), &[], &[], &forks)
        .unwrap()
        .state;
    assert!(s
        .step_with_verdicts(&c, &block(22), &[], &[], &forks)
        .is_err());
    let out = s
        .step_with_verdicts(&c, &block(22), &[], &[v], &forks)
        .unwrap();
    assert_eq!(out.state.problems[&p].winner, Some(b));
    assert!(out.state.m3.as_ref().unwrap().pending_rechecks.is_empty());
}

#[test]
fn t53_problem_void_keeps_queued_siblings_and_their_records_informational() {
    use boole_derivation::{m2::cancel_hash, m3::*};
    use scenario::*;
    let c = m3_cfg();
    let (mut s, p) = opened(&c);
    let (a, ab, ap, _) = candidate(&c, &mut s, p, 7);
    let (b, bb, bp, _) = candidate(&c, &mut s, p, 8);
    until(&mut s, &c, 15);
    run(
        &mut s,
        &c,
        vec![
            input(3, open(&ab, cancel_hash(&[8; 32])), 0),
            input(3, open(&bb, cancel_hash(&[8; 32])), 1),
        ],
    );
    run(
        &mut s,
        &c,
        vec![
            input(4, part(a, 0, &[], &ap[0]), 0),
            input(4, part(b, 0, &[], &bp[0]), 1),
        ],
    );
    run(&mut s, &c, vec![]);
    let records = vec![
        verdict(0, a, Outcome::Void, 0x0801, 0),
        verdict(1, b, Outcome::Accept, 0, 0),
    ];
    let out = s
        .step_with_verdicts(
            &c,
            &block(19),
            &[],
            &records,
            &ForkTable::genesis(&c).unwrap(),
        )
        .unwrap();
    assert!(out.state.problems[&p].voided);
    assert_eq!(out.state.candidates[&(p, a)].status() as u8, 9);
    assert_eq!(out.state.candidates[&(p, b)].status() as u8, 9);
    let m = out.state.m3.as_ref().unwrap();
    assert_eq!(m.queue.len(), 2);
    assert_eq!(m.next_applied, 2);
    assert!(m.rewards.is_empty());
    assert_eq!(out.verdicts, records);
}

#[test]
fn t55_real_service_void_checkpoint_releases_the_same_barrier() {
    use boole_derivation::{
        m2::{cancel_hash, Status},
        m3::*,
    };
    use scenario::*;
    let vector: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m3/service-void-v1.3.json"
    ))
    .unwrap();
    assert_eq!(
        vector["provenance"]["source_sha256"],
        hex::encode(hash(include_bytes!(
            "../../../scripts/zk_registry/service.py"
        )))
    );
    assert_eq!(
        vector["provenance"]["generator_sha256"],
        hex::encode(hash(include_bytes!(
            "../../../scripts/devnet/generate_m3_service_vectors.py"
        )))
    );
    assert_eq!(
        vector["provenance"]["policy_sha256"],
        hex::encode(hash(include_bytes!(
            "../../../scripts/zk_registry/data/error_policy_v1.json"
        )))
    );
    let c = m3_cfg();
    let (mut s, p, a, b, ab) = checking_with_blocker(&c);
    let forks = ForkTable::genesis(&c).unwrap();
    run(
        &mut s,
        &c,
        vec![input(3, open(&ab, cancel_hash(&[8; 32])), 0)],
    );
    s = s
        .step_with_verdicts(
            &c,
            &block(19),
            &[input(4, part(a, 0, &[], &[42; 7]), 0)],
            &[verdict(0, b, Outcome::Accept, 0, 0)],
            &forks,
        )
        .unwrap()
        .state;
    assert_eq!(s.candidates[&(p, a)].status(), Status::Checking);
    assert_eq!(s.candidates[&(p, b)].status(), Status::Passed);
    assert_eq!(
        vector["rows"][0]["statuses"],
        serde_json::json!({"1":"CHECKING","2":"VALID-REVEAL"})
    );
    run(&mut s, &c, vec![]);
    assert!(s
        .step_with_verdicts(&c, &block(21), &[], &[], &forks)
        .is_err());
    assert_eq!(vector["rows"][2]["statuses"]["1"], "QUARANTINED");
    let mut forks = forks;
    forks.rows.push(ForkRow {
        epoch: 1,
        height: 21,
        gate_rule_digest: hash(b"Registry parity quarantine"),
        kind: ForkKind::Quarantine,
        list: vec![ForkItem {
            commit_hash: a,
            disposition: Disposition::Void,
        }],
    });
    let mut release = verdict(1, a, Outcome::Void, QUARANTINE_VOID, 0);
    release.record.gate_epoch = 1;
    let out = s
        .step_with_verdicts(&c, &block(21), &[], &[release], &forks)
        .unwrap();
    let statuses = [(a, "1"), (b, "2")]
        .into_iter()
        .map(|(ch, id)| {
            let name = match out.state.candidates[&(p, ch)].status() {
                Status::Void => "VOID",
                Status::Accepted => "ACCEPTED",
                other => panic!("unexpected checkpoint {other:?}"),
            };
            (id.to_string(), name)
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        serde_json::to_value(statuses).unwrap(),
        vector["rows"][3]["statuses"]
    );
    assert_eq!(out.state.problems[&p].winner, Some(b));
    assert_eq!(vector["rows"][3]["winner"], 2);
    assert!(!out.state.problems[&p].voided);
}

#[test]
fn fork_r4_s14_requires_canonical_kind_lists() {
    use boole_derivation::m3::*;
    let c = scenario::m3_cfg();
    let item = ForkItem {
        commit_hash: [1; 32],
        disposition: Disposition::Recheck,
    };
    for kind in [ForkKind::Genesis, ForkKind::KernelFix, ForkKind::Policy] {
        let mut t = ForkTable::genesis(&c).unwrap();
        if kind == ForkKind::Genesis {
            t.rows[0].list.push(item.clone());
        } else {
            t.rows.push(ForkRow {
                epoch: 1,
                height: 20,
                gate_rule_digest: [2; 32],
                kind,
                list: vec![item.clone()],
            });
        }
        assert_eq!(t.validate(&c), Err(14), "{kind:?} must have an empty list");
    }
    for list in [
        vec![],
        vec![item.clone(), item.clone()],
        vec![
            ForkItem {
                commit_hash: [2; 32],
                disposition: Disposition::Void,
            },
            item.clone(),
        ],
    ] {
        let mut t = ForkTable::genesis(&c).unwrap();
        t.rows.push(ForkRow {
            epoch: 1,
            height: 20,
            gate_rule_digest: [2; 32],
            kind: ForkKind::Quarantine,
            list,
        });
        assert_eq!(t.validate(&c), Err(14));
    }
}

#[test]
fn fork_kernel_fix_activation_enqueues_in_original_order() {
    use boole_derivation::{m2::Status, m3::*};
    use scenario::*;
    let c = m3_cfg();
    let (s, p, a, b) = kernel_fault_and_winner(&c);
    let mut forks = ForkTable::genesis(&c).unwrap();
    forks.rows.push(ForkRow {
        epoch: 1,
        height: 22,
        gate_rule_digest: hash(b"fixed gate"),
        kind: ForkKind::KernelFix,
        list: vec![],
    });
    let out = s
        .step_with_verdicts(&c, &block(22), &[], &[], &forks)
        .unwrap();
    assert_eq!(out.events[0], 8);
    assert_eq!(&out.events[1..5], &1u32.to_be_bytes());
    assert_eq!(&out.events[5..37], &hash(b"fixed gate"));
    assert_eq!(out.events[37], 1);
    let m = out.state.m3.as_ref().unwrap();
    assert_eq!(m.queue.len(), 4);
    assert_eq!(m.queue[2].key, (p, b));
    assert_eq!(m.queue[3].key, (p, a));
    for (i, j) in m.queue[2..].iter().enumerate() {
        assert_eq!(
            (j.queue_index, j.kind, j.avail, j.epoch_override),
            (2 + i as u64, RecordKind::Recheck, 22, Some(1))
        );
    }
    assert_eq!(m.pending_rechecks, [(p, a), (p, b)].into());
    assert_eq!(m.fork_history, forks.rows);
    let s = out
        .state
        .step_with_verdicts(&c, &block(23), &[], &[], &forks)
        .unwrap()
        .state;
    assert!(s
        .step_with_verdicts(&c, &block(24), &[], &[], &forks)
        .is_err());
    let mut records = vec![
        verdict(2, b, Outcome::Reject, 0x0301, 0),
        verdict(3, a, Outcome::Accept, 0, 0),
    ];
    for v in &mut records {
        v.record.kind = RecordKind::Recheck;
        v.record.gate_epoch = 1;
    }
    let out = s
        .step_with_verdicts(&c, &block(24), &[], &records, &forks)
        .unwrap();
    assert_eq!(out.state.candidates[&(p, b)].status(), Status::Stale);
    assert_eq!(out.state.candidates[&(p, a)].status(), Status::Accepted);
    let m = out.state.m3.as_ref().unwrap();
    assert!(m.pending_rechecks.is_empty());
    assert!(m.admissions[&1].retired);
    assert!(m.rewards[&1].compensation);
}

#[test]
fn fork_quarantine_releases_at_original_index_and_binds_disposition() {
    use boole_derivation::{
        m2::{cancel_hash, Status},
        m3::*,
    };
    use scenario::*;
    let c = m3_cfg();
    let (mut s, p, a, b, ab) = checking_with_blocker(&c);
    run(
        &mut s,
        &c,
        vec![input(3, open(&ab, cancel_hash(&[8; 32])), 0)],
    );
    let genesis = ForkTable::genesis(&c).unwrap();
    s = s
        .step_with_verdicts(
            &c,
            &block(19),
            &[input(4, part(a, 0, &[], &[42; 7]), 0)],
            &[verdict(0, b, Outcome::Accept, 0, 0)],
            &genesis,
        )
        .unwrap()
        .state;
    run(&mut s, &c, vec![]);
    assert!(s
        .step_with_verdicts(&c, &block(21), &[], &[], &genesis)
        .is_err());
    for disposition in [Disposition::Recheck, Disposition::Void] {
        let mut t = genesis.clone();
        t.rows.push(ForkRow {
            epoch: 1,
            height: 21,
            gate_rule_digest: hash(b"quarantine gate"),
            kind: ForkKind::Quarantine,
            list: vec![ForkItem {
                commit_hash: a,
                disposition,
            }],
        });
        let mut v = verdict(1, a, Outcome::Accept, 0, 0);
        if disposition == Disposition::Void {
            v.record.outcome = Outcome::Void;
            v.record.reason = QUARANTINE_VOID;
        }
        assert!(
            s.step_with_verdicts(&c, &block(21), &[], &[v.clone()], &t)
                .is_err(),
            "old epoch refused"
        );
        v.record.gate_epoch = 1;
        if disposition == Disposition::Void {
            let mut bad = v.clone();
            bad.record.outcome = Outcome::Accept;
            bad.record.reason = 0;
            assert!(s
                .step_with_verdicts(&c, &block(21), &[], &[bad], &t)
                .is_err());
        }
        let out = s
            .step_with_verdicts(&c, &block(21), &[], &[v.clone()], &t)
            .unwrap();
        assert_eq!(out.events[0], 8);
        assert_eq!(out.events[70], 5, "activation precedes verdict");
        assert_eq!(out.verdicts, vec![v]);
        let m = out.state.m3.as_ref().unwrap();
        assert_eq!((m.queue.len(), m.next_applied), (2, 2));
        assert_eq!(
            (m.queue[1].queue_index, m.queue[1].avail, m.queue[1].kind),
            (1, 19, RecordKind::Verdict)
        );
        assert_eq!(
            out.state.problems[&p].winner,
            Some(if disposition == Disposition::Void {
                b
            } else {
                a
            })
        );
        assert_eq!(
            out.state.candidates[&(p, a)].status(),
            if disposition == Disposition::Void {
                Status::Void
            } else {
                Status::Accepted
            }
        );
        assert!(!out.state.problems[&p].voided);
        assert!(m
            .rewards
            .values()
            .all(|r| r.key.1 != a || disposition == Disposition::Recheck));
        for ch in [b, [0; 32]] {
            let mut bad = t.clone();
            bad.rows[1].list[0].commit_hash = ch;
            assert!(s
                .step_with_verdicts(&c, &block(21), &[], &[], &bad)
                .is_err());
        }
    }
    assert_eq!(s.height(), 20);
    assert_eq!(s.candidates[&(p, a)].status(), Status::Checking);
}

#[test]
fn fork_r4_bytes_and_three_profile_roots_match_independent_python() {
    use boole_derivation::{
        m2::{Config, State},
        m3::*,
    };
    use scenario::*;
    let v: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/devnet-m3/system-v1.3.json")).unwrap();
    for row in v["forks"].as_array().unwrap() {
        let kind = match row["kind"].as_u64().unwrap() {
            0 => ForkKind::Genesis,
            1 => ForkKind::KernelFix,
            2 => ForkKind::Quarantine,
            _ => ForkKind::Policy,
        };
        let r = ForkRow {
            epoch: row["epoch"].as_u64().unwrap() as u32,
            height: 1,
            gate_rule_digest: hex::decode(row["gate_rule_digest"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap(),
            kind,
            list: row["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| ForkItem {
                    commit_hash: hex::decode(i["commit_hash"].as_str().unwrap())
                        .unwrap()
                        .try_into()
                        .unwrap(),
                    disposition: if i["disposition"] == 1 {
                        Disposition::Recheck
                    } else {
                        Disposition::Void
                    },
                })
                .collect(),
        };
        assert_eq!(
            hex::encode(r.list_digest().unwrap()),
            row["list_digest"].as_str().unwrap()
        );
        assert_eq!(
            hex::encode(r.activation_record().unwrap()),
            row["body"].as_str().unwrap()
        );
    }
    for (i, mut c) in [
        Config::devnet(),
        Config::lean_testnet(720927),
        Config::production(720928),
    ]
    .into_iter()
    .enumerate()
    {
        c.m3 = Some(if i == 0 {
            M3Config::devnet(c.slot_table.len())
        } else {
            M3Config::lean_testnet(c.slot_table.len())
        });
        let mut t = ForkTable::genesis(&c).unwrap();
        t.rows.push(ForkRow {
            epoch: 1,
            height: 1,
            gate_rule_digest: hash(b"policy gate"),
            kind: ForkKind::Policy,
            list: vec![],
        });
        let s = State::new(&c, if i == 0 { 30_000_000 } else { 60_000_000 }).unwrap();
        let out = s.step_with_verdicts(&c, &block(1), &[], &[], &t).unwrap();
        assert_eq!(out.events.len(), 70);
        assert_eq!(
            hex::encode(out.state.root),
            v["profiles"][i]["policy_root_1"].as_str().unwrap()
        );
    }
}

#[test]
fn fork_quarantine_epoch_override_preserves_a_fuel_delayed_queue() {
    use boole_derivation::{
        m2::{cancel_hash, Status},
        m3::*,
    };
    use scenario::*;
    let c = m3_cfg();
    let (mut s, p) = opened(&c);
    let (a, ab, ap, _) = candidate(&c, &mut s, p, 7);
    let (b, bb, bp, _) = candidate(&c, &mut s, p, 8);
    until(&mut s, &c, 15);
    run(
        &mut s,
        &c,
        vec![
            input(3, open(&ab, cancel_hash(&[8; 32])), 0),
            input(3, open(&bb, cancel_hash(&[8; 32])), 1),
        ],
    );
    run(
        &mut s,
        &c,
        vec![
            input(4, part(b, 0, &[], &bp[0]), 0),
            input(4, part(a, 0, &[], &ap[0]), 1),
        ],
    );
    run(&mut s, &c, vec![]);
    let genesis = ForkTable::genesis(&c).unwrap();
    let heavy = verdict(0, b, Outcome::Accept, 0, 3000);
    for h in 19..21 {
        s = s
            .step_with_verdicts(&c, &block(h), &[], std::slice::from_ref(&heavy), &genesis)
            .unwrap()
            .state;
    }
    assert!(s
        .step_with_verdicts(&c, &block(21), &[], std::slice::from_ref(&heavy), &genesis)
        .is_err());
    let mut t = genesis;
    t.rows.push(ForkRow {
        epoch: 1,
        height: 21,
        gate_rule_digest: hash(b"delayed quarantine"),
        kind: ForkKind::Quarantine,
        list: vec![ForkItem {
            commit_hash: a,
            disposition: Disposition::Void,
        }],
    });
    let mut release = verdict(1, a, Outcome::Void, QUARANTINE_VOID, 0);
    release.record.gate_epoch = 1;
    let out = s
        .step_with_verdicts(&c, &block(21), &[], &[heavy, release], &t)
        .unwrap();
    assert_eq!(out.state.problems[&p].winner, Some(b));
    assert_eq!(out.state.candidates[&(p, a)].status(), Status::Void);
    let m = out.state.m3.as_ref().unwrap();
    assert_eq!(m.queue[0].epoch_override, None);
    assert_eq!(m.queue[1].epoch_override, Some(1));
    assert_eq!(
        m.queue
            .iter()
            .map(|j| (j.queue_index, j.avail))
            .collect::<Vec<_>>(),
        vec![(0, 17), (1, 17)]
    );
    assert_eq!(m.next_applied, 2);
}

#[test]
fn fork_full_genesis_replay_restart_and_changed_history_qualification() {
    use boole_derivation::{
        m2::{cancel_hash, State, Status},
        m3::*,
    };
    use scenario::*;
    let c = m3_cfg();
    let (mut preparation, p) = opened(&c);
    let (a, ab, _, _) = candidate(&c, &mut preparation, p, 7);
    let (b, bb, _, _) = candidate(&c, &mut preparation, p, 8);
    for quarantine in [false, true] {
        let mut forks = ForkTable::genesis(&c).unwrap();
        if quarantine {
            forks.rows.push(ForkRow {
                epoch: 1,
                height: 21,
                gate_rule_digest: hash(b"release gate"),
                kind: ForkKind::Quarantine,
                list: vec![ForkItem {
                    commit_hash: a,
                    disposition: Disposition::Void,
                }],
            });
        }
        let fix_epoch = if quarantine { 2 } else { 1 };
        forks.rows.push(ForkRow {
            epoch: fix_epoch,
            height: 22,
            gate_rule_digest: hash(b"fixed gate"),
            kind: ForkKind::KernelFix,
            list: vec![],
        });
        forks.rows.push(ForkRow {
            epoch: fix_epoch + 1,
            height: 25,
            gate_rule_digest: hash(b"policy gate"),
            kind: ForkKind::Policy,
            list: vec![],
        });
        let mut tape = Vec::new();
        for h in 1..=30 {
            let items = match h {
                1 => vec![input(
                    8,
                    pool(
                        &c,
                        pool_leaf(c.chain_id, 2, &[], b"generator-v1", &[9; 32]),
                        1,
                        3,
                        1,
                    ),
                    0,
                )],
                5 => vec![input(
                    9,
                    problem_body(0, 1, 0, &[], [9; 32], &[], b"generator-v1"),
                    0,
                )],
                6 => vec![input(2, commit(p, a), 0)],
                7 => vec![input(2, commit(p, b), 0)],
                16 => vec![input(3, open(&bb, cancel_hash(&[8; 32])), 0)],
                17 => vec![input(4, part(b, 0, &[], &[42; 8]), 0)],
                18 => vec![input(3, open(&ab, cancel_hash(&[8; 32])), 0)],
                19 => vec![input(4, part(a, 0, &[], &[42; 7]), 0)],
                _ => vec![],
            };
            let mut records = match h {
                19 => vec![verdict(0, b, Outcome::Accept, 0, 0)],
                21 => vec![verdict(
                    1,
                    a,
                    if quarantine {
                        Outcome::Void
                    } else {
                        Outcome::Reject
                    },
                    if quarantine {
                        QUARANTINE_VOID
                    } else {
                        KERNEL_FAULT
                    },
                    0,
                )],
                24 => {
                    if quarantine {
                        vec![verdict(2, b, Outcome::Accept, 0, 0)]
                    } else {
                        vec![
                            verdict(2, b, Outcome::Reject, 0x0301, 0),
                            verdict(3, a, Outcome::Accept, 0, 0),
                        ]
                    }
                }
                _ => vec![],
            };
            if h == 21 && quarantine {
                records[0].record.gate_epoch = 1;
            }
            if h == 24 {
                for r in &mut records {
                    r.record.kind = RecordKind::Recheck;
                    r.record.gate_epoch = fix_epoch;
                }
            }
            tape.push((h, items, records));
        }
        let mut baseline = State::new(&c, 30_000_000).unwrap();
        let mut replay = State::new(&c, 30_000_000).unwrap();
        let mut restart = None;
        for (h, items, records) in &tape {
            let out = baseline
                .step_with_verdicts(&c, &block(*h), items, records, &forks)
                .unwrap();
            let copy = replay
                .step_with_verdicts(&c, &block(*h), items, records, &forks)
                .unwrap();
            assert_eq!(out.events, copy.events);
            assert_eq!(out.state.root, copy.state.root);
            assert_eq!(
                verdict_deposits(&c, &block(*h), &out.verdicts).unwrap(),
                verdict_deposits(&c, &block(*h), &copy.verdicts).unwrap()
            );
            assert_eq!(
                out.payments
                    .iter()
                    .map(|p| reward_deposit(&c, &block(*h), p).unwrap())
                    .collect::<Vec<_>>(),
                copy.payments
                    .iter()
                    .map(|p| reward_deposit(&c, &block(*h), p).unwrap())
                    .collect::<Vec<_>>()
            );
            if *h == 22 {
                restart = Some(out.state.clone());
            }
            baseline = out.state;
            replay = copy.state;
        }
        let mut restarted = restart.unwrap();
        for (h, items, records) in &tape {
            if *h > 22 {
                restarted = restarted
                    .step_with_verdicts(&c, &block(*h), items, records, &forks)
                    .unwrap()
                    .state;
            }
        }
        assert_eq!(baseline.root, restarted.root);
        assert_eq!(
            baseline.candidates[&(p, b)].status(),
            if quarantine {
                Status::Accepted
            } else {
                Status::Stale
            }
        );
        let m = baseline.m3.as_ref().unwrap();
        assert_eq!(m.fork_history, forks.rows);
        assert!(m.pending_rechecks.is_empty());
        assert_eq!(m.next_applied, m.queue.len());
        assert_eq!(m.rewards.len(), if quarantine { 1 } else { 2 });
        let before = baseline.root;
        let mut changed = forks.clone();
        changed.rows[1].gate_rule_digest[0] ^= 1;
        assert!(baseline
            .step_with_verdicts(&c, &block(31), &[], &[], &changed)
            .is_err());
        let mut wrong = State::new(&c, 30_000_000).unwrap();
        let mut diverged = false;
        for (h, items, records) in &tape {
            let out = wrong
                .step_with_verdicts(&c, &block(*h), items, records, &changed)
                .unwrap();
            if *h == changed.rows[1].height {
                let mut original = State::new(&c, 30_000_000).unwrap();
                for (n, i, r) in &tape {
                    if n > h {
                        break;
                    }
                    original = original
                        .step_with_verdicts(&c, &block(*n), i, r, &forks)
                        .unwrap()
                        .state;
                }
                assert_ne!(out.state.root, original.root);
                diverged = true;
                break;
            }
            wrong = out.state;
        }
        assert!(diverged);
        assert_eq!(baseline.root, before);
    }
}

#[test]
fn fork_quarantine_void_remains_bound_until_a_future_result_applies() {
    use boole_derivation::{
        m2::{cancel_hash, Status},
        m3::*,
    };
    use scenario::*;
    let c = m3_cfg();
    let (mut s, p, a, b, ab) = checking_with_blocker(&c);
    run(
        &mut s,
        &c,
        vec![input(3, open(&ab, cancel_hash(&[8; 32])), 0)],
    );
    let mut forks = ForkTable::genesis(&c).unwrap();
    s = s
        .step_with_verdicts(
            &c,
            &block(19),
            &[input(4, part(a, 0, &[], &[42; 7]), 0)],
            &[verdict(0, b, Outcome::Accept, 0, 0)],
            &forks,
        )
        .unwrap()
        .state;
    run(&mut s, &c, vec![]);
    forks.rows.push(ForkRow {
        epoch: 1,
        height: 21,
        gate_rule_digest: hash(b"future VOID"),
        kind: ForkKind::Quarantine,
        list: vec![ForkItem {
            commit_hash: a,
            disposition: Disposition::Void,
        }],
    });
    let mut v = verdict(1, a, Outcome::Void, QUARANTINE_VOID, 10000);
    v.record.gate_epoch = 1;
    for h in 21..30 {
        let out = s
            .step_with_verdicts(&c, &block(h), &[], std::slice::from_ref(&v), &forks)
            .unwrap();
        assert!(out.verdicts.is_empty());
        assert_eq!(out.state.candidates[&(p, a)].status(), Status::Checking);
        assert_eq!(out.state.m3.as_ref().unwrap().queue.len(), 2);
        s = out.state;
        if h == 21 {
            let mut bad = v.clone();
            bad.record.outcome = Outcome::Accept;
            bad.record.reason = 0;
            assert!(s
                .step_with_verdicts(&c, &block(22), &[], &[bad], &forks)
                .is_err());
        }
    }
    let out = s
        .step_with_verdicts(&c, &block(30), &[], &[v], &forks)
        .unwrap();
    assert_eq!(out.state.problems[&p].winner, Some(b));
    assert_eq!(out.state.candidates[&(p, a)].status(), Status::Void);
    assert!(out.state.m3.as_ref().unwrap().quarantine_voids.is_empty());
}
