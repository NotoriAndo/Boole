//! N5.3 M4 tracer — the supported `boole node start --network testnet`
//! interface must keep a fresh node live-but-not-ready while its configured
//! loopback bootstrap is absent, then synchronize the exact named-network
//! block and open readiness after that bootstrap returns.
//!
//! This is deliberately ignored in the ordinary crate test loop because it
//! starts three real node processes and executes the pinned Lean checker.
//! It uses only a committed synthetic fixture, numeric loopback endpoints and
//! per-test temporary state. It is not public-network mining and consumes no
//! public-eligible problem inventory.

use boole_core::SigningKeyV2;
use boole_testkit::rand_suffix;
use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const NETWORK_ID: &str = "boole-testnet-2";
const OWNER_DEV_ID: &str = "testnet2-smoke-owner-v1";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn fixture_path() -> PathBuf {
    repo_root().join("fixtures/protocol/runtime-smoke/testnet2-lenbound-share.v1.json")
}

fn scenario_path() -> PathBuf {
    repo_root().join("fixtures/protocol/runtime-smoke/testnet2-pinned-highrate.v1.json")
}

fn checker_path() -> PathBuf {
    repo_root().join("lean/checker")
}

fn cli_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_boole-cli"))
}

fn node_binary() -> PathBuf {
    cli_binary()
        .parent()
        .expect("boole-cli binary parent")
        .join("boole-node")
}

fn build_pinned_checker_inputs() {
    // A clean checkout has no gitignored `.olean` for this imported helper.
    // Building only `boole_check` leaves the checker executable present but
    // makes every real proof fail with "unknown module prefix Boole". Keep
    // this multiprocess test independently runnable while matching the
    // self-test lane's precondition exactly.
    let output = Command::new("lake")
        .current_dir(checker_path())
        .args(["build", "Boole.Family.V0Helpers", "boole_check"])
        .output()
        .expect("run pinned checker build");
    assert!(
        output.status.success(),
        "pinned checker build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn reserve_loopback_ports(count: usize) -> Vec<u16> {
    let listeners = (0..count)
        .map(|_| TcpListener::bind("127.0.0.1:0").expect("reserve loopback port"))
        .collect::<Vec<_>>();
    let ports = listeners
        .iter()
        .map(|listener| listener.local_addr().expect("reserved address").port())
        .collect();
    drop(listeners);
    ports
}

struct RunningNode {
    label: String,
    child: Child,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

/// Own every child and temporary path so a panic cannot leave a node running.
struct Harness {
    root: PathBuf,
    children: Vec<RunningNode>,
}

impl Harness {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "boole-cli-n53-m4-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        fs::create_dir_all(&root).expect("create M4 temp root");
        Self {
            root,
            children: Vec::new(),
        }
    }

    fn data_dir(&self, label: &str) -> PathBuf {
        self.root.join(label)
    }

    fn spawn_node(
        &mut self,
        label: &str,
        data_dir: &Path,
        http_port: u16,
        p2p_port: Option<u16>,
        bootstrap_ports: &[u16],
    ) {
        assert!(
            !bootstrap_ports.is_empty(),
            "controlled testnet nodes need an explicit bootstrap"
        );
        let stdout_path = self.root.join(format!("{label}.out"));
        let stderr_path = self.root.join(format!("{label}.err"));
        let stdout = File::create(&stdout_path).expect("create node stdout");
        let stderr = File::create(&stderr_path).expect("create node stderr");

        let mut command = Command::new(cli_binary());
        command
            .current_dir(repo_root())
            .arg("node")
            .arg("start")
            .arg("--network")
            .arg("testnet")
            .arg("--port")
            .arg(http_port.to_string())
            .arg("--data-dir")
            .arg(data_dir)
            .arg("--scenario")
            .arg(scenario_path())
            .arg("--lean-checker-dir")
            .arg(checker_path())
            .env("BOOLE_NODE_BIN", node_binary())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        if let Some(port) = p2p_port {
            command.arg("--p2p-listen").arg(format!("127.0.0.1:{port}"));
        }
        for port in bootstrap_ports {
            command
                .arg("--bootstrap-peer")
                .arg(format!("127.0.0.1:{port}"));
        }

        let child = command.spawn().expect("spawn supported boole node start");
        self.children.push(RunningNode {
            label: label.to_string(),
            child,
            stdout_path,
            stderr_path,
        });
    }

    fn assert_running(&mut self, label: &str) {
        let node = self
            .children
            .iter_mut()
            .find(|node| node.label == label)
            .unwrap_or_else(|| panic!("unknown running node {label}"));
        if let Some(status) = node.child.try_wait().expect("query child status") {
            panic!(
                "node {label} exited early with {status}\nstdout:\n{}\nstderr:\n{}",
                fs::read_to_string(&node.stdout_path).unwrap_or_default(),
                fs::read_to_string(&node.stderr_path).unwrap_or_default()
            );
        }
    }

    fn stop_node(&mut self, label: &str) {
        let index = self
            .children
            .iter()
            .position(|node| node.label == label)
            .unwrap_or_else(|| panic!("unknown running node {label}"));
        let mut node = self.children.swap_remove(index);
        if node.child.try_wait().expect("query child status").is_none() {
            node.child.kill().expect("kill node");
        }
        node.child.wait().expect("reap node");
    }

    fn diagnostics(&self) -> String {
        self.children
            .iter()
            .map(|node| {
                format!(
                    "\n== {} stdout ==\n{}\n== {} stderr ==\n{}",
                    node.label,
                    fs::read_to_string(&node.stdout_path).unwrap_or_default(),
                    node.label,
                    fs::read_to_string(&node.stderr_path).unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        for node in &mut self.children {
            if node.child.try_wait().ok().flatten().is_none() {
                let _ = node.child.kill();
            }
            let _ = node.child.wait();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct HttpResponse {
    status: u16,
    body: Vec<u8>,
    json: Value,
}

fn raw_http_request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<(u16, Vec<u8>), String> {
    let body = body
        .map(serde_json::to_vec)
        .transpose()
        .map_err(|error| error.to_string())?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500))
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(|error| error.to_string())?;
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    if let Some(body) = body.as_ref() {
        request.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|error| error.to_string())?;
    if let Some(body) = body {
        stream.write_all(&body).map_err(|error| error.to_string())?;
    }

    let mut raw = Vec::new();
    match stream.read_to_end(&mut raw) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::ConnectionReset && !raw.is_empty() => {}
        Err(error) => return Err(error.to_string()),
    }
    let separator = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| format!("HTTP response has no header/body boundary: {raw:?}"))?;
    let headers = std::str::from_utf8(&raw[..separator]).map_err(|error| error.to_string())?;
    let status = headers
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| format!("HTTP response has no status: {headers}"))?
        .parse::<u16>()
        .map_err(|error| error.to_string())?;
    Ok((status, raw[(separator + 4)..].to_vec()))
}

fn http_request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<HttpResponse, String> {
    let (status, response_body) = raw_http_request(addr, method, path, body)?;
    let json = serde_json::from_slice(&response_body)
        .map_err(|error| format!("HTTP body is not JSON: {error}; body={response_body:?}"))?;
    Ok(HttpResponse {
        status,
        body: response_body,
        json,
    })
}

fn get(addr: SocketAddr, path: &str) -> Result<HttpResponse, String> {
    http_request(addr, "GET", path, None)
}

fn metric_value(addr: SocketAddr, name: &str) -> u64 {
    let (status, body) = raw_http_request(addr, "GET", "/metrics", None)
        .unwrap_or_else(|error| panic!("GET http://{addr}/metrics failed: {error}"));
    assert_eq!(status, 200, "metrics endpoint must remain available");
    let body = std::str::from_utf8(&body).expect("metrics response UTF-8");
    let prefix = format!("{name} ");
    body.lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("metric {name} not found in:\n{body}"))
        .parse()
        .unwrap_or_else(|error| panic!("metric {name} is not a u64: {error}"))
}

fn post(addr: SocketAddr, path: &str, body: &Value) -> HttpResponse {
    http_request(addr, "POST", path, Some(body)).unwrap_or_else(|error| {
        panic!("POST http://{addr}{path} failed: {error}");
    })
}

fn wait_live(harness: &mut Harness, label: &str, addr: SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(180);
    while Instant::now() < deadline {
        harness.assert_running(label);
        if let Ok(response) = get(addr, "/live") {
            if response.status == 200 && response.json["ok"] == true {
                return;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "node {label} never became live at {addr}:{}",
        harness.diagnostics()
    );
}

fn registration_envelope(fixture: &Value) -> Value {
    let owner = SigningKeyV2::from_dev_id(OWNER_DEV_ID);
    assert_eq!(
        fixture["sessionState"]["ownerPk"],
        owner.pk_hex(),
        "fixture owner must match the deterministic M4 key"
    );
    let valid_before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("wall clock")
        .as_secs()
        + 300;
    let payload = json!({
        "schema": "boole.sessions.register.v1",
        "session": fixture["sessionState"].clone(),
        "currentHeight": 0,
        "validBefore": valid_before,
        "nonce": format!("m4-register-{}", rand_suffix()),
    });
    let signed = owner
        .sign_for_network(&payload, Some(NETWORK_ID))
        .expect("sign network-scoped session registration");
    json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
        "network_id": signed.network_id,
    })
}

fn authorized_submission(fixture: &Value) -> Value {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("wall clock")
        .as_millis();
    json!({
        "body": fixture["body"].clone(),
        "canonTag": 0,
        "ts": u64::try_from(now_ms).expect("timestamp fits u64"),
        "session": fixture["submissionSession"].clone(),
    })
}

fn assert_waiting_for_bootstrap(addr: SocketAddr) {
    let live = get(addr, "/live").expect("live response");
    assert_eq!(live.status, 200, "node must remain live: {}", live.json);
    assert_eq!(live.json["ok"], true);

    let ready = get(addr, "/ready").expect("ready response");
    assert_eq!(
        ready.status, 503,
        "an absent bootstrap must keep readiness closed: {}",
        ready.json
    );
    assert_eq!(ready.json["reason"], "p2p_head_not_synced");
    assert_eq!(ready.json["checks"]["p2p_head_synced"], false);
}

fn wait_for_controlled_convergence(
    harness: &mut Harness,
    nodes: &[(&str, SocketAddr)],
    expected_c: &str,
    expected_genesis_spec_hash: &str,
    expected_block_body: &[u8],
) {
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut last = String::new();
    let mut first_converged_at = None;
    while Instant::now() < deadline {
        for (label, _) in nodes {
            harness.assert_running(label);
        }
        let observed = nodes
            .iter()
            .map(|(label, addr)| {
                let status = get(*addr, "/status")?;
                let ready = get(*addr, "/ready")?;
                let block = get(*addr, "/block/latest")?;
                Ok::<_, String>((*label, status, ready, block))
            })
            .collect::<Result<Vec<_>, _>>();
        if let Ok(observed) = observed {
            last = observed
                .iter()
                .map(|(label, status, ready, block)| {
                    format!(
                        "{label}: height={} c={} ready={} blockStatus={}",
                        status.json["height"], status.json["c"], ready.status, block.status
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            let converged = observed.iter().all(|(_, status, ready, block)| {
                status.status == 200
                    && status.json["height"] == 1
                    && status.json["c"] == expected_c
                    && status.json["genesisSpecHash"] == expected_genesis_spec_hash
                    && status.json["replayMatchesRuntime"] == true
                    && ready.status == 200
                    && ready.json["ok"] == true
                    && ready.json["checks"]["p2p_head_synced"] == true
                    && block.status == 200
                    && block.body == expected_block_body
            });
            if converged {
                let since = first_converged_at.get_or_insert_with(Instant::now);
                // Hold the exact state longer than the configured five-second
                // inter-cycle sleep. One fleeting matching read must not
                // qualify this healthy-loopback M4 topology.
                if since.elapsed() >= Duration::from_secs(6) {
                    return;
                }
            } else {
                first_converged_at = None;
            }
        }
        thread::sleep(Duration::from_millis(200));
    }
    panic!(
        "three controlled CLI nodes did not converge stably: {last}{}",
        harness.diagnostics()
    );
}

fn assert_manifest_network_and_genesis(data_dirs: &[&Path], expected_genesis_spec_hash: &str) {
    for data_dir in data_dirs {
        let path = data_dir.join("state.manifest.json");
        let manifest: Value = serde_json::from_slice(
            &fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
        )
        .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
        assert_eq!(
            manifest["network_id"],
            NETWORK_ID,
            "{} must stay pinned to the testnet preset",
            path.display()
        );
        assert_eq!(
            manifest["genesis_hash"],
            expected_genesis_spec_hash,
            "{} must retain the same compiled genesis",
            path.display()
        );
    }
}

#[test]
#[ignore = "needs-multiprocess"]
fn join_testnet_syncs_from_bootstrap_to_seed_block_head() {
    build_pinned_checker_inputs();
    let mut harness = Harness::new();
    let ports = reserve_loopback_ports(7);
    let seed_http = ports[0];
    let http_a = ports[1];
    let http_b = ports[2];
    let http_c = ports[3];
    let p2p_a = ports[4];
    let p2p_b = ports[5];
    let p2p_c = ports[6];
    let data_a = harness.data_dir("node-a");
    let data_b = harness.data_dir("node-b");
    let data_c = harness.data_dir("node-c");

    // Create exactly one deterministic, session-authorized testnet block on
    // A through the supported CLI + HTTP surface. B and C are deliberately down,
    // so A cannot obtain a false-positive bootstrap-ready observation.
    harness.spawn_node("a-seed", &data_a, seed_http, None, &[p2p_b, p2p_c]);
    let seed_addr: SocketAddr = format!("127.0.0.1:{seed_http}")
        .parse()
        .expect("seed HTTP address");
    wait_live(&mut harness, "a-seed", seed_addr);

    let fixture: Value = serde_json::from_slice(&fs::read(fixture_path()).expect("share fixture"))
        .expect("share fixture JSON");
    assert_eq!(
        fixture["submissionSession"]["signedWork"]["network_id"], NETWORK_ID,
        "the setup share must be scoped to the controlled testnet"
    );
    let registered = post(seed_addr, "/sessions", &registration_envelope(&fixture));
    assert_eq!(
        registered.status, 200,
        "fixture session registration failed: {}",
        registered.json
    );
    assert_eq!(registered.json["ok"], true);

    let submitted = post(seed_addr, "/submit", &authorized_submission(&fixture));
    assert_eq!(
        submitted.status, 200,
        "synthetic fixture submission failed: {}",
        submitted.json
    );
    assert_eq!(submitted.json["accepted"], true);
    assert_eq!(submitted.json["height"], 1);
    assert_eq!(submitted.json["block"]["height"], 0);
    assert_eq!(submitted.json["replayMatchesRuntime"], true);

    let seed_status = get(seed_addr, "/status").expect("seed status");
    assert_eq!(seed_status.status, 200);
    assert_eq!(seed_status.json["height"], 1);
    let expected_c = seed_status.json["c"]
        .as_str()
        .expect("seed head c")
        .to_string();
    let expected_genesis_spec_hash = seed_status.json["genesisSpecHash"]
        .as_str()
        .expect("seed genesis spec hash")
        .to_string();
    let seed_block = get(seed_addr, "/block/latest").expect("seed latest block");
    assert_eq!(seed_block.status, 200);
    let expected_block_body = seed_block.body;

    harness.stop_node("a-seed");

    // Start the two empty joiners while their sole configured bootstrap is
    // absent. Liveness must open, but readiness must remain a typed 503.
    harness.spawn_node("b", &data_b, http_b, Some(p2p_b), &[p2p_a]);
    harness.spawn_node("c", &data_c, http_c, Some(p2p_c), &[p2p_a]);
    let addr_b: SocketAddr = format!("127.0.0.1:{http_b}").parse().expect("B HTTP");
    let addr_c: SocketAddr = format!("127.0.0.1:{http_c}").parse().expect("C HTTP");
    wait_live(&mut harness, "b", addr_b);
    wait_live(&mut harness, "c", addr_c);
    assert_waiting_for_bootstrap(addr_b);
    assert_waiting_for_bootstrap(addr_c);

    // Restart A from its durable one-block data directory. The final three
    // processes all enter through `boole node start --network testnet` and
    // use explicit numeric-loopback bootstrap endpoints only.
    harness.spawn_node("a", &data_a, http_a, Some(p2p_a), &[p2p_b, p2p_c]);
    let addr_a: SocketAddr = format!("127.0.0.1:{http_a}").parse().expect("A HTTP");
    wait_live(&mut harness, "a", addr_a);

    wait_for_controlled_convergence(
        &mut harness,
        &[("a", addr_a), ("b", addr_b), ("c", addr_c)],
        &expected_c,
        &expected_genesis_spec_hash,
        &expected_block_body,
    );
    assert_manifest_network_and_genesis(
        &[data_a.as_path(), data_b.as_path(), data_c.as_path()],
        &expected_genesis_spec_hash,
    );
    for (label, addr) in [("B", addr_b), ("C", addr_c)] {
        let applied = metric_value(addr, "boole_p2p_ingress_blocks_ingested_total")
            + metric_value(addr, "boole_p2p_sync_blocks_applied_total");
        assert_eq!(
            applied, 1,
            "{label} must apply the single block exactly once across ingress and sync"
        );
    }

    // Harness::Drop reaps every process and deletes the temporary state even
    // when an assertion above panics. Nothing here exposes public P2P,
    // activates mining/rewards, or consumes non-fixture work.
}
