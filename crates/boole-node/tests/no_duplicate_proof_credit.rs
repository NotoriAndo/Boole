//! N2.3 — `/submit` rejects a duplicate proof / cross-pk farming.
//!
//! The same canonical proof package, resubmitted under a different prover pk,
//! must be credited at most once. The dedup key is the SERVER's
//! `SHA-256(proof package bytes)` (recomputed from `body["bytes"]`), never a
//! client-supplied field, so two miners cannot farm one proof for two credits.
//!
//! The attack is constructed here from the `multiminer.v1.json` steps: the
//! test copies step 0's proof `bytes` into step 1's body (N4-pre.1 made the
//! fixture's own steps carry distinct proofs, so the duplicate is forged
//! test-side), giving the same proof under a different `pk`, each valid at
//! the runtime head it sees. Step 0 is admitted at genesis and credits its
//! pk; step 1 re-presents the same proof under a second pk at the new head.
//! Without dedup it produces a second block and a second credit; with dedup
//! it is rejected `duplicate_proof` before any write, so only one credit
//! accrues. (Consensus-level dedup — ADR-0012 — would also refuse the block
//! itself now; this test pins the admission-layer early-reject cache.)

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn scenario_path() -> PathBuf {
    repo_root().join("fixtures/protocol/runtime-smoke/v1.json")
}

fn multiminer_steps() -> Vec<Value> {
    let raw =
        fs::read_to_string(repo_root().join("fixtures/protocol/runtime-smoke/multiminer.v1.json"))
            .expect("multiminer fixture");
    let doc: Value = serde_json::from_str(&raw).expect("multiminer json");
    doc["steps"].as_array().expect("steps array").clone()
}

/// The `/submit` envelope for a multiminer step: the inner share body plus the
/// `canonTag`/`ts` the runtime reads from the top level. `ts` is kept as-is so
/// the produced block — and therefore the next head — is deterministic and
/// matches the head each later step's proof-of-work was generated against.
fn submit_envelope(step: &Value) -> Value {
    json!({
        "body": step["body"].clone(),
        "canonTag": step["canonTag"].clone(),
        "ts": step["ts"].clone(),
    })
}

struct Boot {
    addr: SocketAddr,
    dir: PathBuf,
    handle: thread::JoinHandle<anyhow::Result<()>>,
}

fn boot(max_requests: usize) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-n23-proof-dedup-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("tmp dir");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let block_path = dir.join("blocks.ndjson");
    let rewards = dir.join("rewards.ndjson");
    let dedup = dir.join("proof-dedup.ndjson");
    let scenario = scenario_path();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path,
                reward_ledger_path: Some(rewards),
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                operator_signer_pks: vec![],
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                signed_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                proof_dedup_ledger_path: Some(dedup),
                max_requests: Some(max_requests),
                genesis_override: None,
                state_dir: None,
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit: true,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    Boot { addr, dir, handle }
}

fn http_request(addr: SocketAddr, raw: &str) -> (u16, Value) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    stream.write_all(raw.as_bytes()).expect("write");
    let mut buf = Vec::new();
    match stream.read_to_end(&mut buf) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::ConnectionReset && !buf.is_empty() => {}
        Err(err) => panic!("read response: {err}"),
    }
    let text = String::from_utf8(buf).expect("utf8 response");
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("status not parseable: {text}"));
    let (_, body_text) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("response missing body: {text}"));
    let value: Value = serde_json::from_str(body_text)
        .unwrap_or_else(|err| panic!("body not JSON: {err}, raw={body_text}"));
    (status, value)
}

fn http_post(addr: SocketAddr, path: &str, body: &Value) -> (u16, Value) {
    let body_str = serde_json::to_string(body).expect("body json");
    let raw = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
        body_str.len()
    );
    http_request(addr, &raw)
}

fn http_get(addr: SocketAddr, path: &str) -> (u16, Value) {
    let raw = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    http_request(addr, &raw)
}

fn balance(addr: SocketAddr, pk: &str) -> String {
    let (_, body) = http_get(addr, &format!("/account/{pk}/balance"));
    body["balance"].as_str().unwrap_or("").to_string()
}

#[test]
fn same_proof_under_two_pks_credits_once() {
    let steps = multiminer_steps();
    // Four served requests below: submit step0, submit step1, then two balance
    // GETs. `boot_with` stops the server after `max_requests`, so the count
    // must match exactly or the final `join()` hangs (an over-count) / a later
    // request is refused (an under-count).
    let boot = boot(4);

    // Step 0: a proof admitted at genesis, crediting its pk.
    let pk0 = steps[0]["body"]["pk"].as_str().expect("pk0").to_string();
    let (s0, v0) = http_post(boot.addr, "/submit", &submit_envelope(&steps[0]));
    assert_eq!(s0, 200, "step0 status: {v0}");
    assert_eq!(v0["accepted"], true, "step0 must be admitted: {v0}");
    let head = v0["c"].as_str().expect("new head from step0").to_string();

    // Step 1: the SAME proof bytes under a different pk (forged here by
    // copying step 0's bytes — the fixture's steps carry distinct proofs
    // since N4-pre.1), at the new head so its proof-of-work is valid and
    // the reject can only come from the dedup guard.
    let pk1 = steps[1]["body"]["pk"].as_str().expect("pk1").to_string();
    assert_ne!(pk0, pk1, "fixture must carry two distinct pks");
    let mut step1 = submit_envelope(&steps[1]);
    step1["body"]["c"] = json!(head);
    step1["body"]["bytes"] = steps[0]["body"]["bytes"].clone();
    let (_s1, v1) = http_post(boot.addr, "/submit", &step1);
    assert_ne!(
        v1["accepted"],
        json!(true),
        "the same proof under a second pk must not be credited again: {v1}"
    );

    // Exactly one credit: pk0 paid, pk1 not.
    assert_ne!(
        balance(boot.addr, &pk0),
        "0",
        "first pk must be credited once"
    );
    assert_eq!(
        balance(boot.addr, &pk1),
        "0",
        "second pk must not be credited for the duplicate proof"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&boot.dir);
}

#[test]
fn proof_dedup_key_is_server_computed_not_client_field() {
    let steps = multiminer_steps();
    let boot = boot(2);

    let (_s0, v0) = http_post(boot.addr, "/submit", &submit_envelope(&steps[0]));
    assert_eq!(v0["accepted"], true, "step0 must be admitted: {v0}");
    let head = v0["c"].as_str().expect("new head").to_string();

    // Step 1 differs from step 0 in pk AND n/j/nonceS (the share identity);
    // only `bytes` is shared (copied from step 0 — the fixture steps are
    // distinct since N4-pre.1). The reject therefore proves the dedup key is
    // the server's hash of the proof bytes, not the share hash, the pk, or
    // any client-varying field.
    let mut step1 = submit_envelope(&steps[1]);
    step1["body"]["c"] = json!(head);
    step1["body"]["bytes"] = steps[0]["body"]["bytes"].clone();
    assert_ne!(steps[0]["body"]["n"], steps[1]["body"]["n"], "n differs");
    assert_ne!(steps[0]["body"]["j"], steps[1]["body"]["j"], "j differs");
    let (_s1, v1) = http_post(boot.addr, "/submit", &step1);

    assert_eq!(
        v1["reason"], "duplicate_proof",
        "the second submit must be the typed duplicate-proof reject: {v1}"
    );
    assert_ne!(
        v1["accepted"],
        json!(true),
        "duplicate proof must not be accepted: {v1}"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&boot.dir);
}
