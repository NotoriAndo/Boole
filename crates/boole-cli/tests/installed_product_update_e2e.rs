//! Real-process closed-local update/rollback/recovery E2E.
//!
//! The test uses the actual `boole-cli` binary, a loopback-only byte server,
//! and deterministic non-production KAT roots. It never boots a guest and
//! grants no production, mining, reward, consensus, P2P or activation power.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::{
    read_installed_curl_product_state, GuestArtifactRole, ProductArtifactRole,
    CURL_PRODUCT_INSTALLED_GUEST_DIRECTORY, CURL_PRODUCT_INSTALLED_MANIFEST_FILE,
    CURL_PRODUCT_INSTALL_STATE_FILE, CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY,
};
use boole_testkit::{
    write_bootable_curl_product_kat_metadata, BootableCurlProductKatInput,
    BootableCurlProductKatRelease, BootableCurlProductKatRoots,
};
use sha2::{Digest, Sha256};

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "boole-installed-product-update-e2e-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create E2E fixture root");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct StaticLoopbackServer {
    port: u16,
    shutdown: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
    handle: Option<JoinHandle<()>>,
}

impl StaticLoopbackServer {
    fn start(bundle: &Path) -> Self {
        let mut routes = BTreeMap::new();
        for entry in fs::read_dir(bundle).expect("read bundle") {
            let entry = entry.expect("bundle entry");
            if entry.file_type().expect("bundle type").is_file() {
                routes.insert(
                    format!("/{}", entry.file_name().to_string_lossy()),
                    fs::read(entry.path()).expect("bundle bytes"),
                );
            }
        }
        let guest = bundle.join("guest");
        for entry in fs::read_dir(&guest).expect("read guest bundle") {
            let entry = entry.expect("guest entry");
            routes.insert(
                format!("/guest/{}", entry.file_name().to_string_lossy()),
                fs::read(entry.path()).expect("guest bytes"),
            );
        }
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback server");
        let port = listener.local_addr().expect("loopback address").port();
        let shutdown = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let thread_shutdown = Arc::clone(&shutdown);
        let thread_requests = Arc::clone(&requests);
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if thread_shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { break };
                serve_connection(stream, &routes, &thread_requests);
            }
        });
        Self {
            port,
            shutdown,
            requests,
            handle: Some(handle),
        }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("request log").clone()
    }
}

impl Drop for StaticLoopbackServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn serve_connection(
    mut stream: TcpStream,
    routes: &BTreeMap<String, Vec<u8>>,
    requests: &Arc<Mutex<Vec<String>>>,
) {
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !raw.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => raw.extend_from_slice(&chunk[..read]),
            Err(_) => return,
        }
    }
    let request = String::from_utf8_lossy(&raw);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    requests.lock().expect("request log").push(path.to_string());
    let (status, reason, body) = match routes.get(path) {
        Some(body) => (200, "OK", body.as_slice()),
        None => (404, "Not Found", &[][..]),
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

fn digest(path: &Path) -> String {
    hex::encode(Sha256::digest(fs::read(path).expect("digest input")))
}

fn build_bundle(
    fixture: &FixtureDir,
    label: &str,
    release: BootableCurlProductKatRelease,
) -> (PathBuf, BootableCurlProductKatRoots) {
    let sources = fixture.join(&format!("{label}-sources"));
    fs::create_dir(&sources).expect("source dir");
    let mut product_artifacts = BTreeMap::new();
    for role in [
        ProductArtifactRole::HostCli,
        ProductArtifactRole::HostNode,
        ProductArtifactRole::HostWalletAgent,
        ProductArtifactRole::HostController,
    ] {
        let path = sources.join(role.as_str());
        fs::write(&path, format!("{label}:product:{}", role.as_str())).expect("product source");
        product_artifacts.insert(role, path);
    }
    let mut guest_artifacts = BTreeMap::new();
    for role in GuestArtifactRole::DIRECT_BOOT_ALL {
        let path = sources.join(role.as_str());
        fs::write(&path, format!("{label}:guest:{}", role.as_str())).expect("guest source");
        guest_artifacts.insert(role, path);
    }
    let bundle = fixture.join(&format!("{label}-bundle"));
    let roots = write_bootable_curl_product_kat_metadata(BootableCurlProductKatInput {
        output_dir: bundle.clone(),
        source_revision: "56".repeat(20),
        product_artifacts: product_artifacts.clone(),
        guest_artifacts: guest_artifacts.clone(),
        release,
    })
    .expect("signed KAT metadata");
    let guest = bundle.join("guest");
    fs::create_dir(&guest).expect("guest transport dir");
    for (role, source) in product_artifacts {
        fs::copy(source, bundle.join(role.as_str())).expect("product transport copy");
    }
    for (role, source) in guest_artifacts {
        fs::copy(source, guest.join(role.as_str())).expect("guest transport copy");
    }
    (bundle, roots)
}

fn run_cli(args: &[String]) -> (serde_json::Value, Output) {
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(args)
        .output()
        .expect("run real CLI process");
    let bytes = if output.status.success() {
        &output.stdout
    } else {
        &output.stderr
    };
    let value: serde_json::Value = serde_json::from_slice(bytes).unwrap_or_else(|error| {
        panic!(
            "CLI output is not JSON ({error}): stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output)
}

fn root_args(roots: &BootableCurlProductKatRoots) -> Vec<String> {
    vec![
        "--product-trust-root-key-id".into(),
        roots.product_key_id.clone(),
        "--product-trust-root-public-key".into(),
        roots.product_public_key_hex.clone(),
        "--guest-trust-root-key-id".into(),
        roots.guest_key_id.clone(),
        "--guest-trust-root-public-key".into(),
        roots.guest_public_key_hex.clone(),
    ]
}

fn install_args(
    base_url: &str,
    install_root: &Path,
    staging: &Path,
    roots: &BootableCurlProductKatRoots,
) -> Vec<String> {
    let mut args = vec![
        "product".into(),
        "install-direct-boot".into(),
        "--base-url".into(),
        base_url.into(),
        "--install-root".into(),
        install_root.display().to_string(),
        "--download-staging".into(),
        staging.display().to_string(),
        "--first-product-minimum".into(),
        "1".into(),
        "--first-guest-minimum".into(),
        "1".into(),
        "--timeout-seconds".into(),
        "5".into(),
    ];
    args.extend(root_args(roots));
    args
}

fn lifecycle_args(
    command: &str,
    install_root: &Path,
    roots: &BootableCurlProductKatRoots,
) -> Vec<String> {
    let mut args = vec![
        "product".into(),
        command.into(),
        "--install-root".into(),
        install_root.display().to_string(),
    ];
    args.extend(root_args(roots));
    args
}

#[test]
fn real_cli_update_rollback_corrupt_recovery_and_reset_preserve_security_state() {
    let fixture = FixtureDir::new();
    let first_release = BootableCurlProductKatRelease::default();
    let (first_bundle, roots) = build_bundle(&fixture, "first", first_release);
    let first_product_digest = digest(&first_bundle.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE));
    let first_guest_digest = digest(&first_bundle.join("guest-update-manifest"));
    let second_release = BootableCurlProductKatRelease {
        product_sequence: 2,
        product_version: "0.0.1-installed-mac-e2e-kat".to_string(),
        product_previous_manifest_sha256: Some(first_product_digest),
        guest_sequence: 2,
        guest_version: "0.0.1-installed-mac-e2e-kat".to_string(),
        guest_previous_manifest_sha256: Some(first_guest_digest),
    };
    let (second_bundle, second_roots) = build_bundle(&fixture, "second", second_release);
    assert_eq!(roots, second_roots, "both releases use the same KAT roots");

    let install_root = fixture.join("install");
    let staging = fixture.join("download-staging");
    let first_server = StaticLoopbackServer::start(&first_bundle);
    let (first, first_output) = run_cli(&install_args(
        &first_server.base_url(),
        &install_root,
        &staging,
        &roots,
    ));
    assert!(first_output.status.success(), "first install: {first}");
    assert_eq!(first["command"], "product.install-direct-boot");
    assert_eq!(first["result"]["releaseSequence"], 1);
    assert_eq!(first["result"]["guestReleaseSequence"], 1);
    assert_eq!(first_server.requests().len(), 19);
    drop(first_server);

    let second_server = StaticLoopbackServer::start(&second_bundle);
    let (second, second_output) = run_cli(&install_args(
        &second_server.base_url(),
        &install_root,
        &staging,
        &roots,
    ));
    assert!(second_output.status.success(), "signed update: {second}");
    assert_eq!(second["result"]["releaseSequence"], 2);
    assert_eq!(second["result"]["guestReleaseSequence"], 2);
    assert_eq!(second_server.requests().len(), 19);
    drop(second_server);

    let (rollback, rollback_output) = run_cli(&lifecycle_args(
        "rollback-direct-boot",
        &install_root,
        &roots,
    ));
    assert!(rollback_output.status.success(), "rollback: {rollback}");
    assert_eq!(rollback["result"]["activeReleaseSequence"], 1);
    assert_eq!(rollback["result"]["releaseFloorSequence"], 2);
    assert_eq!(rollback["result"]["guestReleaseFloorSequence"], 2);
    assert_eq!(rollback["result"]["rollbackReleaseSequence"], 2);

    let state_before_replay =
        fs::read(install_root.join(CURL_PRODUCT_INSTALL_STATE_FILE)).expect("state before replay");
    let replay_server = StaticLoopbackServer::start(&second_bundle);
    let (replay, replay_output) = run_cli(&install_args(
        &replay_server.base_url(),
        &install_root,
        &staging,
        &roots,
    ));
    assert!(
        !replay_output.status.success(),
        "accepted update replay: {replay}"
    );
    assert_eq!(replay["error"]["reason"], "release-rejected");
    assert_eq!(
        replay_server.requests().len(),
        2,
        "replay must stop after the signed manifest and signature"
    );
    assert_eq!(
        fs::read(install_root.join(CURL_PRODUCT_INSTALL_STATE_FILE)).expect("state after replay"),
        state_before_replay
    );
    drop(replay_server);

    let rolled_back = read_installed_curl_product_state(&install_root)
        .expect("state readable")
        .expect("state present");
    let corrupt_path = install_root
        .join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY)
        .join(rolled_back.version_directory())
        .join(CURL_PRODUCT_INSTALLED_GUEST_DIRECTORY)
        .join(GuestArtifactRole::GuestRootDisk.as_str());
    fs::write(&corrupt_path, b"deliberately corrupt active generation")
        .expect("corrupt active generation");

    let (recovered, recovery_output) = run_cli(&lifecycle_args(
        "recover-direct-boot",
        &install_root,
        &roots,
    ));
    assert!(recovery_output.status.success(), "recovery: {recovered}");
    assert_eq!(recovered["result"]["activeReleaseSequence"], 2);
    assert_eq!(recovered["result"]["releaseFloorSequence"], 2);
    assert_eq!(recovered["result"]["guestReleaseFloorSequence"], 2);
    assert!(recovered["result"]["rollbackReleaseSequence"].is_null());

    let state_root = fixture.join("state");
    fs::create_dir(&state_root).expect("state root");
    fs::set_permissions(&state_root, fs::Permissions::from_mode(0o700)).expect("state mode");
    for name in ["controller", "host", "journal"] {
        let path = state_root.join(name);
        fs::create_dir(&path).expect("state child");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("child mode");
    }
    fs::write(state_root.join("controller/residue"), b"controller").expect("controller residue");
    fs::write(state_root.join("host/residue"), b"host").expect("host residue");
    fs::write(
        state_root.join("journal/replay.ndjson"),
        b"durable-journal\n",
    )
    .expect("journal");
    fs::write(
        state_root.join("wallet-state-must-survive"),
        b"wallet-sentinel",
    )
    .expect("wallet sentinel");
    let (reset, reset_output) = run_cli(&[
        "product".into(),
        "reset-direct-boot".into(),
        "--state-root".into(),
        state_root.display().to_string(),
    ]);
    assert!(reset_output.status.success(), "runtime reset: {reset}");
    assert_eq!(reset["result"]["controllerRemoved"], true);
    assert_eq!(reset["result"]["hostRuntimeRemoved"], true);
    assert_eq!(reset["result"]["journalPreserved"], true);
    assert_eq!(reset["result"]["walletStateTouched"], false);
    assert_eq!(
        fs::read(state_root.join("journal/replay.ndjson")).expect("journal survives"),
        b"durable-journal\n"
    );
    assert_eq!(
        fs::read(state_root.join("wallet-state-must-survive")).expect("wallet survives"),
        b"wallet-sentinel"
    );
}
