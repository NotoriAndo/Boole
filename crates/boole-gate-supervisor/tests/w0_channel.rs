#![cfg(feature = "w0-fixture")]
use boole_lean_runner::w0_types::*;
use std::collections::BTreeMap;
fn parents() -> BTreeMap<String, String> {
    ["user", "net", "pid", "mnt", "ipc", "uts"]
        .into_iter()
        .map(|k| (k.into(), "parent".into()))
        .collect()
}
fn capabilities() -> Capabilities {
    Capabilities {
        namespaces: parents().into_keys().map(|k| (k, "child".into())).collect(),
        landlock_abi: 4,
        landlock_enforced: true,
        seccomp: 2,
        apparmor: "boole-gate-bwrap (unconfined)".into(),
        dumpable: 0,
        caps_zero: true,
        pid_one: true,
    }
}
fn prepared() -> Vec<u8> {
    let mut c = capabilities();
    c.landlock_enforced = false;
    c.dumpable = 1;
    let mut raw = serde_json::to_vec(&Event::Prepared {
        nonce: "request-1".into(),
        capabilities: c,
    })
    .unwrap();
    raw.push(b'\n');
    raw
}
fn ready() -> Vec<u8> {
    let mut b = serde_json::to_vec(&Event::Ready {
        nonce: "request-1".into(),
        capabilities: capabilities(),
    })
    .unwrap();
    b.push(b'\n');
    [prepared(), b].concat()
}
#[test]
fn private_channel_controls_reject_missing_foreign_incomplete_and_reordered_evidence() {
    let parent = parents();
    let first = ready();
    assert!(parse_channel(&first, "request-1", &parent, true).is_ok());
    assert!(parse_channel(&first, "request-1", &parent, false).is_err());
    assert!(parse_channel(&first, "request-2", &parent, true).is_err());
    let mut full = first.clone();
    let footer = serde_json::to_vec(&Event::Finished {
        nonce: "request-1".into(),
        code: 0,
        reaped: 2,
    })
    .unwrap();
    full.extend(&footer);
    full.push(b'\n');
    assert_eq!(
        parse_channel(&full, "request-1", &parent, false)
            .unwrap()
            .reaped,
        2
    );
    let mut reversed = footer.clone();
    reversed.push(b'\n');
    reversed.extend(&first);
    assert!(parse_channel(&reversed, "request-1", &parent, true).is_err());
    let mut duplicate = first.clone();
    duplicate.extend(&first);
    assert!(parse_channel(&duplicate, "request-1", &parent, true).is_err());
    for raw in [
        vec![],
        b"not json\n".to_vec(),
        full[..full.len() - 1].to_vec(),
        [full.clone(), b"{}\n".to_vec()].concat(),
    ] {
        assert!(parse_channel(&raw, "request-1", &parent, true).is_err());
    }
}
#[test]
fn private_channel_rejects_every_missing_layer_and_duplicate_shadow_key() {
    for i in 0..9 {
        let mut c = capabilities();
        match i {
            0 => c.landlock_abi = 3,
            1 => c.seccomp = 0,
            2 => c.apparmor = "unconfined".into(),
            3 => c.dumpable = 1,
            4 => c.caps_zero = false,
            5 => c.pid_one = false,
            6 => {
                c.namespaces.remove("pid");
            }
            7 => {
                c.namespaces.insert("pid".into(), "parent".into());
            }
            _ => c.landlock_enforced = false,
        }
        let mut raw = serde_json::to_vec(&Event::Ready {
            nonce: "request-1".into(),
            capabilities: c,
        })
        .unwrap();
        raw.push(b'\n');
        assert!(parse_channel(&[prepared(), raw].concat(), "request-1", &parents(), true).is_err());
    }
    let raw = String::from_utf8(ready())
        .unwrap()
        .replace("\"ipc\":\"child\"", "\"ipc\":\"parent\",\"ipc\":\"child\"");
    assert!(parse_channel(raw.as_bytes(), "request-1", &parents(), true).is_err());
}

fn live_frame(value: &serde_json::Value) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let body = serde_json::to_vec(value).unwrap();
    let mut raw = vec![0u8; LIVE_FRAME_SIZE];
    raw[..8].copy_from_slice(b"BW0SNAP3");
    raw[8..16].copy_from_slice(&value["sequence"].as_u64().unwrap().to_be_bytes());
    raw[16..20].copy_from_slice(&(body.len() as u32).to_be_bytes());
    raw[20..20 + body.len()].copy_from_slice(&body);
    let end = raw.len() - 32;
    let digest = Sha256::digest(&raw[..end]);
    raw[end..].copy_from_slice(&digest);
    raw
}
#[test]
fn live_heartbeat_is_grant_bound_and_revocation_and_expiry_fail_closed() {
    use sha2::{Digest, Sha256};
    let g = serde_json::json!({"state":"granted","owner":"N","step":"W0","request_id":"one","generation":1,"start_unix":100,"end_unix":1000});
    let pin: String = Sha256::digest(serde_json::to_vec(&g).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let good = serde_json::json!({"version":3,"sequence":1,"grant":g,"active":true,"tick_unix":200,"closing":false,"observed_monotonic_ns":100000000});
    assert!(validate_live(&live_frame(&good), &pin, 200, 100000001).is_ok());
    for tick in [167, 230] {
        let mut b = good.clone();
        b["tick_unix"] = serde_json::json!(tick);
        assert!(validate_live(&live_frame(&b), &pin, 200, 100000001).is_ok());
    }
    for (key, value) in [
        ("active", serde_json::json!(false)),
        ("tick_unix", serde_json::json!(166)),
        ("tick_unix", serde_json::json!(231)),
        ("grant", serde_json::json!({"state":"granted"})),
    ] {
        let mut b = good.clone();
        b[key] = value;
        assert!(validate_live(&live_frame(&b), &pin, 200, 100000001).is_err());
    }
    assert!(validate_live(&live_frame(&good), &pin, 900, 100000001).is_err());
}

#[test]
fn observer_kill_requires_a_populated_tree_before_the_signal() {
    let mut witness = ObserverDeath {
        sigkill: true,
        members_before_kill: 4,
        automatic_empty: true,
        cleanup_complete: true,
    };
    assert!(witness.qualifies());
    for count in 0..4 {
        witness.members_before_kill = count;
        assert!(!witness.qualifies());
    }
    witness.members_before_kill = 4;
    witness.sigkill = false;
    assert!(!witness.qualifies());
    witness.sigkill = true;
    witness.automatic_empty = false;
    assert!(!witness.qualifies());
    witness.automatic_empty = true;
    witness.cleanup_complete = false;
    assert!(!witness.qualifies());
}

#[test]
fn heartbeat_checksum_padding_and_sequence_refuse_torn_reads() {
    use sha2::{Digest, Sha256};
    let g = serde_json::json!({"state":"granted","start_unix":100,"end_unix":1000});
    let pin: String = Sha256::digest(serde_json::to_vec(&g).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let value = serde_json::json!({"version":3,"sequence":1,"grant":g,"active":true,"tick_unix":200,"closing":false,"observed_monotonic_ns":100000000});
    let good = live_frame(&value);
    assert!(validate_live(&good, &pin, 200, 100000001).is_ok());
    for index in [0, 8, 16, 30, 3000, LIVE_FRAME_SIZE - 1] {
        let mut bad = good.clone();
        bad[index] ^= 1;
        assert!(validate_live(&bad, &pin, 200, 100000001).is_err());
    }
    assert!(validate_live(&good[..good.len() - 1], &pin, 200, 100000001).is_err());
    let mut bad = good;
    bad[3000] = 1;
    let end = bad.len() - 32;
    let hash = Sha256::digest(&bad[..end]);
    bad[end..].copy_from_slice(&hash);
    assert!(validate_live(&bad, &pin, 200, 100000001).is_err());
}
#[test]
fn python_and_rust_decode_identical_fixed_heartbeat_vectors() {
    use sha2::{Digest, Sha256};
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/devnet/w0/heartbeat.py");
    let output = std::process::Command::new("python3")
        .args(["-IB"])
        .arg(script)
        .arg("vector-v3")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let g = serde_json::json!({"state":"granted","owner":"N","step":"W0","request_id":"vector","generation":3,"start_unix":100,"end_unix":1000});
    let pin: String = Sha256::digest(serde_json::to_vec(&g).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let value = serde_json::json!({"version":3,"sequence":7,"grant":g,"active":true,"tick_unix":200,"closing":false,"observed_monotonic_ns":100000000});
    assert_eq!(output.stdout, live_frame(&value));
    assert!(validate_live(&output.stdout, &pin, 200, 100000001).is_ok());
}

#[test]
fn slow_d4_control_cannot_survive_inside_the_retained_run_tree() {
    let root = std::path::Path::new("/retained/run/tmp/case");
    assert!(!scratch_path(root, Action::SlowD4, "owned").starts_with(root));
    assert!(scratch_path(root, Action::Noop, "owned").starts_with(root));
}

#[test]
fn snapshot_freshness_is_local_monotonic_not_host_wall_clock_only() {
    use sha2::{Digest, Sha256};
    let g = serde_json::json!({"state":"granted","start_unix":100,"end_unix":1000});
    let pin: String = Sha256::digest(serde_json::to_vec(&g).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let value = serde_json::json!({"version":3,"sequence":1,"grant":g,"active":true,"tick_unix":200,"closing":false,"observed_monotonic_ns":100000000});
    let raw = live_frame(&value);
    assert!(validate_live(&raw, &pin, 200, 3100000000).is_ok());
    assert!(validate_live(&raw, &pin, 200, 3100000001).is_err());
    assert!(validate_live(&raw, &pin, 200, 99999999).is_err());
}

#[test]
fn userns_sysctl_absent_matches_the_tracked_launcher() {
    let root = boole_gate_supervisor::codec::Scratch::new(&std::env::temp_dir()).unwrap();
    assert_eq!(userns_restriction(&root.0.join("absent")).unwrap(), None);
}
#[test]
fn userns_sysctl_zero_refuses_without_rewrite() {
    let root = boole_gate_supervisor::codec::Scratch::new(&std::env::temp_dir()).unwrap();
    let path = root.0.join("zero");
    std::fs::write(&path, "0\n").unwrap();
    assert!(userns_restriction(&path).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "0\n");
}
#[test]
fn userns_sysctl_one_passes_without_rewrite() {
    let root = boole_gate_supervisor::codec::Scratch::new(&std::env::temp_dir()).unwrap();
    let path = root.0.join("one");
    std::fs::write(&path, "1\n").unwrap();
    assert_eq!(userns_restriction(&path).unwrap(), Some("1".into()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "1\n");
}
