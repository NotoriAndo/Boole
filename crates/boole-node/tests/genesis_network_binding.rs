//! N5.2 (ADR-0014) — per-network genesis binding + mismatch refusal.
//!
//! Three enforcement surfaces, all keyed on `GenesisSpec.hash()` (N5.1):
//! `Hello.genesis_hash` carries the SPEC hash (not the raw chain anchor),
//! so peers that agree on the anchor but differ on any committed
//! parameter refuse to gossip; `state.manifest.json` records the spec
//! hash and boot refuses a state dir written under a foreign genesis; a
//! node booted under a COMPILED network name (`boole-dev`/`boole-testnet`)
//! must match that network's compiled-in preset exactly.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use boole_core::CONSENSUS_RULE_VERSION;
use boole_node::{serve_local_node_with_p2p, LocalNodeConfig, P2pConfig};
use boole_p2p::{Frame, FrameError, HeadSummary, TcpTransport, Transport, PROTOCOL_VERSION};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};
use tokio::sync::Notify;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn scenario_path() -> PathBuf {
    repo_root().join("fixtures/protocol/runtime-smoke/v1.json")
}

fn scenario_genesis_c() -> String {
    let raw = fs::read_to_string(scenario_path()).expect("scenario fixture");
    let doc: Value = serde_json::from_str(&raw).expect("scenario json");
    doc["genesisC"].as_str().expect("genesisC").to_string()
}

struct Boot {
    addr: SocketAddr,
    dir: PathBuf,
    shutdown: Arc<Notify>,
    handle: thread::JoinHandle<anyhow::Result<()>>,
}

/// Boot a node; `state_dir`/`network_id`/`genesis_override`/`p2p_listener`
/// are the knobs the binding tests exercise. The caller inspects the join
/// result for refusal cases.
fn boot(
    tag: &str,
    state_dir: Option<PathBuf>,
    network_id: Option<String>,
    genesis_override: Option<String>,
    p2p_listener: Option<TcpListener>,
) -> Boot {
    boot_with_scenario(
        tag,
        scenario_path(),
        state_dir,
        network_id,
        genesis_override,
        p2p_listener,
    )
}

/// SC.7 — variant taking the scenario path so a test can boot from a
/// patched calibration (e.g. a non-consensus MinShareScoreMultiplier)
/// while keeping the preset-matching genesis.
fn boot_with_scenario(
    tag: &str,
    scenario: PathBuf,
    state_dir: Option<PathBuf>,
    network_id: Option<String>,
    genesis_override: Option<String>,
    p2p_listener: Option<TcpListener>,
) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-n52-binding-{tag}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("tmp dir");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind http");
    let addr = listener.local_addr().expect("http addr");
    let (tx, rx) = mpsc::channel();
    let block_path = dir.join("blocks.ndjson");
    let rewards = dir.join("rewards.ndjson");
    let shutdown = Arc::new(Notify::new());
    let shutdown_for_node = shutdown.clone();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node_with_p2p(
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
                proof_dedup_ledger_path: None,
                max_requests: None,
                genesis_override,
                state_dir,
                network_id,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit: false,
            },
            P2pConfig {
                listener: p2p_listener,
                peers: vec!["127.0.0.1:1".parse().expect("allowlist addr")],
                rate_limit_per_60s: boole_node::DEFAULT_P2P_RATE_LIMIT_PER_60S,
            },
            Some(shutdown_for_node),
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    Boot {
        addr,
        dir,
        shutdown,
        handle,
    }
}

fn stop(boot: Boot) {
    boot.shutdown.notify_one();
    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&boot.dir);
}

fn http_get(addr: SocketAddr, path: &str) -> (u16, Value) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    let raw = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
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
        .unwrap_or(0);
    let (_, body) = text.split_once("\r\n\r\n").expect("body break");
    (status, serde_json::from_str(body).expect("json body"))
}

fn metric_value(addr: SocketAddr, name: &str) -> u64 {
    let mut stream = TcpStream::connect(addr).expect("connect metrics");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    let raw = "GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    stream.write_all(raw.as_bytes()).expect("write");
    let mut buf = Vec::new();
    match stream.read_to_end(&mut buf) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::ConnectionReset && !buf.is_empty() => {}
        Err(err) => panic!("read metrics: {err}"),
    }
    let text = String::from_utf8_lossy(&buf);
    text.lines()
        .find(|line| line.starts_with(name))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn wait_until(what: &str, timeout: Duration, check: impl Fn() -> bool) {
    let deadline = Instant::now() + timeout;
    loop {
        if check() {
            return;
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for {what}");
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn hello_with_genesis(genesis_hash: &str) -> Frame {
    Frame::Hello {
        protocol_version: PROTOCOL_VERSION,
        consensus_rule_version: CONSENSUS_RULE_VERSION,
        network_id: "boole-mvp".to_string(),
        genesis_hash: genesis_hash.to_string(),
        head: HeadSummary {
            height: 0,
            c: scenario_genesis_c(),
        },
    }
}

#[test]
#[ignore = "needs-multiprocess"]
fn node_refuses_peer_with_mismatched_genesis() {
    let p2p = TcpListener::bind("127.0.0.1:0").expect("bind p2p");
    let p2p_addr = p2p.local_addr().expect("p2p addr");
    let b = boot("peer-genesis", None, None, None, Some(p2p));

    // N5.2 semantic upgrade: Hello.genesis_hash carries the SPEC hash.
    let (status, doc) = http_get(b.addr, "/status");
    assert_eq!(status, 200, "status: {doc}");
    let spec_hash = doc["genesisSpecHash"]
        .as_str()
        .expect("status must expose genesisSpecHash")
        .to_string();
    assert_ne!(
        spec_hash,
        scenario_genesis_c(),
        "the spec hash must not be the raw chain anchor"
    );

    // A peer presenting the spec hash completes the handshake.
    let transport = TcpTransport::new();
    let mut conn = transport.connect(&p2p_addr).expect("connect");
    transport
        .send_frame(&mut conn, &hello_with_genesis(&spec_hash))
        .expect("send matching hello");
    match transport.recv_frame(&mut conn).expect("hello reply") {
        Frame::Hello { .. } => {}
        other => panic!("expected a Hello reply for the matching spec hash, got {other:?}"),
    }
    drop(conn);

    // A peer presenting the raw anchor (same chain bytes, uncommitted
    // params) is refused — that was exactly the pre-N5.2 hole.
    let mut conn = transport.connect(&p2p_addr).expect("connect 2");
    transport
        .send_frame(&mut conn, &hello_with_genesis(&scenario_genesis_c()))
        .expect("send anchor-only hello");
    match transport.recv_frame(&mut conn) {
        Err(FrameError::ConnectionClosed) | Err(FrameError::Io(_)) => {}
        other => panic!("raw-anchor Hello must be disconnected, got {other:?}"),
    }
    wait_until(
        "the hello mismatch drop to be counted",
        Duration::from_secs(10),
        || metric_value(b.addr, "boole_p2p_ingress_hello_mismatch_drops_total") >= 1,
    );

    stop(b);
}

#[test]
#[ignore = "needs-multiprocess"]
fn boot_refuses_state_dir_with_foreign_genesis() {
    let state_dir = std::env::temp_dir().join(format!(
        "boole-n52-state-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = fs::remove_dir_all(&state_dir);
    fs::create_dir_all(&state_dir).expect("state dir");

    // First boot writes the manifest with the genesis spec hash.
    let a = boot("state-first", Some(state_dir.clone()), None, None, None);
    stop(a);
    let manifest_path = state_dir.join("state.manifest.json");
    let raw = fs::read_to_string(&manifest_path).expect("manifest");
    let mut manifest: Value = serde_json::from_str(&raw).expect("manifest json");
    let recorded = manifest["genesis_hash"]
        .as_str()
        .expect("manifest must record genesis_hash")
        .to_string();
    assert_eq!(recorded.len(), 64, "genesis_hash is a hex32: {recorded}");

    // Tamper: the state dir now claims a foreign genesis. Boot must refuse.
    manifest["genesis_hash"] = json!("11".repeat(32));
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).expect("serialize"),
    )
    .expect("tamper manifest");

    let b = boot("state-foreign", Some(state_dir.clone()), None, None, None);
    b.shutdown.notify_one();
    let joined = b.handle.join().expect("server thread");
    let err = joined.expect_err("boot must refuse a foreign-genesis state dir");
    assert!(
        err.to_string().contains("genesis_hash"),
        "refusal must name the genesis_hash mismatch: {err}"
    );
    let _ = fs::remove_dir_all(&b.dir);
    let _ = fs::remove_dir_all(&state_dir);
}

#[test]
#[ignore = "needs-multiprocess"]
fn named_network_boot_binds_to_compiled_preset() {
    // `boole-dev` is a compiled-in network: its preset matches the standard
    // runtime-smoke scenario, so booting under that name succeeds...
    let ok = boot("preset-ok", None, Some("boole-dev".to_string()), None, None);
    let (status, doc) = http_get(ok.addr, "/status");
    assert_eq!(status, 200, "status: {doc}");
    stop(ok);

    // ...but a node whose effective genesis diverges from the compiled
    // preset (here: a foreign anchor via --genesis) must refuse to boot
    // under that network name instead of silently forking it.
    let bad = boot(
        "preset-diverged",
        None,
        Some("boole-dev".to_string()),
        Some("22".repeat(32)),
        None,
    );
    bad.shutdown.notify_one();
    let joined = bad.handle.join().expect("server thread");
    let err = joined.expect_err("a diverging genesis must refuse to boot under a compiled network");
    assert!(
        err.to_string().contains("boole-dev"),
        "refusal must name the network: {err}"
    );
    let _ = fs::remove_dir_all(&bad.dir);
}

// SC.7 (4th review) — on a NAMED network the admission floor must be the
// consensus floor: a calibration whose MinShareScoreMultiplier diverges
// from the Tier-2 rule constant refuses to boot (the builder commits the
// constant regardless, so the divergent knob could only skew what this
// node admits away from what every replay enforces). Unnamed/fixture
// runs keep the knob as node-local ops config (ADR-0014 Tier-3).
#[test]
fn named_network_boot_fails_fast_on_non_consensus_multiplier() {
    let raw = fs::read_to_string(scenario_path()).expect("scenario fixture");
    let mut doc: Value = serde_json::from_str(&raw).expect("scenario json");
    doc["cfg"]["MinShareScoreMultiplier"] = serde_json::json!(2.0);
    let dir = std::env::temp_dir().join(format!(
        "boole-sc7-multiplier-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("tmp dir");
    let patched = dir.join("scenario.json");
    fs::write(&patched, serde_json::to_string_pretty(&doc).expect("json")).expect("write");

    let bad = boot_with_scenario(
        "multiplier-diverged",
        patched,
        None,
        Some("boole-dev".to_string()),
        None,
        None,
    );
    bad.shutdown.notify_one();
    let joined = bad.handle.join().expect("server thread");
    let err =
        joined.expect_err("a non-consensus multiplier must refuse to boot on a named network");
    assert!(
        err.to_string().contains("MinShareScoreMultiplier"),
        "refusal must name the knob: {err}"
    );
    let _ = fs::remove_dir_all(&bad.dir);
    let _ = fs::remove_dir_all(&dir);
}

// ===== SC.9b (ADR-0016 (a)/(a-2)) — checker pin + executable toolchain =====
//
// `boole-testnet-2` pins `checker_artifact_hash` in its compiled preset. A
// named-network boot that configures a Lean checker must refuse to come up
// when (1) the local checker sources hash differently from the pin, or
// (2) the released toolchain manifest (RELEASE-MANIFEST.json, the tag +
// SHA256SUMS channel) disagrees with the pin or with the toolchain the
// checker process would ACTUALLY execute (`lake env lean` resolved from the
// package dir). A source-hash match with a different executable toolchain
// is a typed refusal, not a warning.

fn canonical_checker_dir() -> PathBuf {
    repo_root().join("lean").join("checker")
}

fn lake_and_lean_available() -> bool {
    std::process::Command::new("lake")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
        && std::process::Command::new("lean")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
}

/// Copy the canonical checker package (pinned sources + release manifest,
/// no `.lake` build outputs) into a tmp dir the test can tamper with.
fn copy_checker(tag: &str) -> PathBuf {
    let src = canonical_checker_dir();
    let dst = std::env::temp_dir().join(format!(
        "boole-sc9b-checker-{tag}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = fs::remove_dir_all(&dst);
    fs::create_dir_all(dst.join("BooleCheck")).expect("mk BooleCheck");
    fs::create_dir_all(dst.join("Boole/Family")).expect("mk Boole/Family");
    for rel in [
        "lean-toolchain",
        "lakefile.lean",
        "lake-manifest.json",
        "RELEASE-MANIFEST.json",
        "BooleCheck/Main.lean",
        "BooleCheck/Audit.lean",
        "Boole/Family/V0Helpers.lean",
    ] {
        fs::copy(src.join(rel), dst.join(rel)).unwrap_or_else(|err| panic!("copy {rel}: {err}"));
    }
    dst
}

/// Boot under the pinned network name with a configured Lean checker dir.
fn boot_testnet2_with_checker(tag: &str, checker_dir: PathBuf) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-sc9b-boot-{tag}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("tmp dir");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind http");
    let addr = listener.local_addr().expect("http addr");
    let (tx, rx) = mpsc::channel();
    let block_path = dir.join("blocks.ndjson");
    let rewards = dir.join("rewards.ndjson");
    let shutdown = Arc::new(Notify::new());
    let shutdown_for_node = shutdown.clone();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node_with_p2p(
            listener,
            LocalNodeConfig {
                scenario_path: scenario_path(),
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
                proof_dedup_ledger_path: None,
                max_requests: None,
                genesis_override: None,
                state_dir: None,
                network_id: Some("boole-testnet-2".to_string()),
                lean_checker_dir: Some(checker_dir),
                lean_checker_disabled: false,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit: false,
            },
            P2pConfig {
                listener: None,
                peers: vec!["127.0.0.1:1".parse().expect("allowlist addr")],
                rate_limit_per_60s: boole_node::DEFAULT_P2P_RATE_LIMIT_PER_60S,
            },
            Some(shutdown_for_node),
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    Boot {
        addr,
        dir,
        shutdown,
        handle,
    }
}

fn expect_boot_refusal(boot: Boot) -> String {
    let _ = boot.addr;
    boot.shutdown.notify_one();
    let joined = boot.handle.join().expect("server thread");
    let err = joined.expect_err("boot must refuse");
    let _ = fs::remove_dir_all(&boot.dir);
    err.to_string()
}

/// SC.9b — a checker whose sources hash differently from the network's
/// compiled pin must refuse to boot, and the refusal must be the CHECKER
/// gate (differential control: the untampered copy gets past the checker
/// gate and fails on the later genesis gate instead).
#[test]
fn named_network_boot_refuses_on_checker_artifact_hash_mismatch() {
    if !lake_and_lean_available() {
        eprintln!("skipping checker pin boot test: lake/lean unavailable");
        return;
    }
    // Tampered copy: any byte change to a pinned source moves the hash.
    let tampered = copy_checker("tampered");
    let main_path = tampered.join("BooleCheck/Main.lean");
    let mut main_text = fs::read_to_string(&main_path).expect("read Main.lean");
    main_text.push_str("\n-- tampered\n");
    fs::write(&main_path, main_text).expect("tamper Main.lean");

    let err = expect_boot_refusal(boot_testnet2_with_checker("mismatch", tampered.clone()));
    assert!(
        err.contains("checker_artifact_hash"),
        "refusal must name the checker pin: {err}"
    );
    let _ = fs::remove_dir_all(&tampered);

    // Differential control: the untampered copy passes the checker gate;
    // this harness's genesis still diverges from the testnet-2 preset, so
    // the boot fails on the GENESIS gate — proving the refusal above was
    // the checker pin, not an unrelated boot failure.
    let pristine = copy_checker("pristine");
    let err = expect_boot_refusal(boot_testnet2_with_checker("control", pristine.clone()));
    assert!(
        !err.contains("checker_artifact_hash") && err.contains("genesis"),
        "pristine checker must clear the checker gate (and fail later on \
         genesis divergence in this harness): {err}"
    );
    let _ = fs::remove_dir_all(&pristine);
}

/// SC.9b — a source-hash match with a different executable Lean identity is
/// a typed refusal: the release manifest declares the toolchain the pin was
/// released with, and boot compares it to what the checker process would
/// actually run (`lake env lean` in the package dir).
#[test]
fn named_network_boot_rejects_wrong_lean_version_or_githash() {
    if !lake_and_lean_available() {
        eprintln!("skipping lean identity boot test: lake/lean unavailable");
        return;
    }
    let checker = copy_checker("wrong-lean");
    let manifest_path = checker.join("RELEASE-MANIFEST.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest"))
            .expect("manifest json");
    manifest["leanGithash"] = json!("0000000000000000000000000000000000000000");
    manifest["leanVersion"] = json!("9.9.9");
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).expect("json"),
    )
    .expect("doctor manifest");

    let err = expect_boot_refusal(boot_testnet2_with_checker("wrong-lean", checker.clone()));
    assert!(
        err.contains("lean") && (err.contains("githash") || err.contains("version")),
        "refusal must name the lean toolchain identity mismatch: {err}"
    );
    let _ = fs::remove_dir_all(&checker);
}

/// SC.9b — same contract for the Lake executable identity.
#[test]
fn named_network_boot_rejects_wrong_lake_version() {
    if !lake_and_lean_available() {
        eprintln!("skipping lake identity boot test: lake/lean unavailable");
        return;
    }
    let checker = copy_checker("wrong-lake");
    let manifest_path = checker.join("RELEASE-MANIFEST.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest"))
            .expect("manifest json");
    manifest["lakeVersion"] = json!("0.0.0-bogus");
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).expect("json"),
    )
    .expect("doctor manifest");

    let err = expect_boot_refusal(boot_testnet2_with_checker("wrong-lake", checker.clone()));
    assert!(
        err.contains("lake"),
        "refusal must name the lake toolchain identity mismatch: {err}"
    );
    let _ = fs::remove_dir_all(&checker);
}

/// SC.10-iv-0 — a node's effective genesis must be ABLE to match a compiled
/// preset that pins seed binding and a checker. Those are network identity
/// declarations no calibration scenario can express, so `genesis_spec` must
/// adopt them from the compiled preset; before this slice they were
/// hardcoded (`false`/`None`/`None`), which made the boot genesis gate
/// unpassable for `boole-testnet-2` under ANY scenario — the checker-pinned
/// live path was structurally unbootable (the pristine-checker control
/// above proves boot then died on the genesis gate). The Tier-1 threshold
/// side of the gate keeps its meaning: a scenario whose
/// t_block/t_share/k_max/retarget diverge from the preset must still refuse.
#[test]
fn genesis_spec_can_match_checker_pinned_preset_identity() {
    // A calibration whose Tier-1 thresholds agree with the presets (both
    // compiled presets share t_block/t_share/k_max); only the retarget
    // schedule and the identity fields under test differ between them.
    let fixture: Value =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");
    let testnet = boole_core::network_genesis_preset("boole-testnet-2").expect("testnet-2 preset");
    let mut cfg = fixture["cfg"].clone();
    cfg["T_block"] = json!(testnet.params.t_block.clone());
    cfg["T_share"] = json!(testnet.params.t_share.clone());
    cfg["K_max"] = json!(testnet.params.k_max);
    let report: boole_core::CalibrationReport =
        serde_json::from_value(cfg).expect("patched cfg parses");
    let base_config =
        boole_node::RuntimeConfig::from_calibration_report(report, 60_000).expect("runtime config");

    // Control: the non-pinned compiled preset (`boole-dev`, identity fields
    // false/None) matched under the previous hardcoded defaults and must
    // keep matching after the preset-aware change.
    let dev = boole_core::network_genesis_preset("boole-dev").expect("dev preset");
    let dev_spec = base_config.genesis_spec("boole-dev", &dev.initial_state.genesis_c);
    assert_eq!(
        dev_spec.hash().to_hex(),
        dev.hash().to_hex(),
        "a threshold-matching node must keep matching the boole-dev preset"
    );

    // The checker-pinned preset: seed_binding_required=true and a Some(...)
    // checker pin come from the PRESET, not the scenario.
    let runtime_config = base_config
        .with_difficulty_retarget(
            testnet
                .params
                .retarget
                .clone()
                .expect("testnet-2 preset declares a retarget schedule"),
        )
        .expect("retarget config");
    let spec = runtime_config.genesis_spec("boole-testnet-2", &testnet.initial_state.genesis_c);
    assert_eq!(
        spec.hash().to_hex(),
        testnet.hash().to_hex(),
        "genesis_spec must adopt the compiled preset's identity fields \
         (seed_binding_required / checker_artifact_hash / family_manifest_root) \
         so a threshold-matching node can boot the checker-pinned network"
    );
}

/// SC.9b — repo-level release-channel consistency (P3.6 subset: tag +
/// SHA256SUMS): the compiled testnet preset pin, the release manifest, the
/// SHA256SUMS file, and the actual checker sources in this repo must all
/// agree, so the pin an operator verifies from the release channel is the
/// pin the network enforces.
#[test]
fn preset_pin_matches_released_checker_toolchain_manifest() {
    let preset = boole_core::network_genesis_preset("boole-testnet-2").expect("testnet-2 preset");
    let pinned = preset
        .params
        .checker_artifact_hash
        .expect("SC.9b flips the testnet checker pin to Some(<hash>) — ADR-0014 (e) resolved");

    let dir = canonical_checker_dir();
    let recomputed =
        boole_lean_runner::checker_artifact_hash(&dir).expect("recompute checker artifact hash");
    assert_eq!(
        pinned, recomputed,
        "the compiled preset pin must equal the repo checker's artifact hash"
    );

    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(dir.join("RELEASE-MANIFEST.json")).expect("release manifest exists"),
    )
    .expect("manifest json");
    assert_eq!(manifest["schema"], "boole.checker.release.v1");
    assert_eq!(
        manifest["checkerArtifactHash"].as_str().expect("hash"),
        pinned,
        "release manifest must declare the pinned artifact hash"
    );
    for key in ["tag", "leanVersion", "leanGithash", "lakeVersion"] {
        assert!(
            manifest[key].as_str().is_some_and(|v| !v.is_empty()),
            "release manifest must declare {key}"
        );
    }

    // SHA256SUMS covers the manifest and every pinned source file, so the
    // minimal release channel (git tag + this file) lets an operator verify
    // a downloaded checker byte-for-byte.
    let sums = fs::read_to_string(dir.join("SHA256SUMS")).expect("SHA256SUMS exists");
    let mut covered = std::collections::BTreeSet::new();
    for line in sums.lines().filter(|l| !l.trim().is_empty()) {
        let (digest, rel) = line
            .split_once("  ")
            .unwrap_or_else(|| panic!("malformed SHA256SUMS line: {line}"));
        let bytes = fs::read(dir.join(rel)).unwrap_or_else(|err| panic!("read {rel}: {err}"));
        let actual = {
            use sha2::{Digest, Sha256};
            hex::encode(Sha256::digest(&bytes))
        };
        assert_eq!(digest, actual, "SHA256SUMS digest drift for {rel}");
        covered.insert(rel.to_string());
    }
    for required in [
        "RELEASE-MANIFEST.json",
        "lean-toolchain",
        "lakefile.lean",
        "lake-manifest.json",
        "BooleCheck/Main.lean",
        "BooleCheck/Audit.lean",
        "Boole/Family/V0Helpers.lean",
    ] {
        assert!(
            covered.contains(required),
            "SHA256SUMS must cover {required}"
        );
    }
}
