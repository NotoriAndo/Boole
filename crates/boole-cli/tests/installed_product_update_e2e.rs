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
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use boole_cli::installed_product_lifecycle::acquire_installed_product_mutation_lease;
use boole_core::{
    canonicalize, read_installed_curl_product_state, GuestArtifactRole, ProductArtifactRole,
    SigningKeyV2, CURL_PRODUCT_INSTALLED_GUEST_DIRECTORY, CURL_PRODUCT_INSTALLED_MANIFEST_FILE,
    CURL_PRODUCT_INSTALL_STATE_FILE, CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY,
    OPERATIONAL_RELEASE_TRUST_POLICY_SIGNING_CONTEXT, OPERATIONAL_RELEASE_TRUST_STATE_FILE,
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

const POLICY_RECOVERY_A: &str = "non-production-installed-policy-recovery-kat-a";
const POLICY_RECOVERY_B: &str = "non-production-installed-policy-recovery-kat-b";
const POLICY_RECOVERY_C: &str = "non-production-installed-policy-recovery-kat-c";
const POLICY_RECOVERY_D: &str = "non-production-installed-policy-recovery-kat-d";

fn policy_key(id: &str) -> SigningKeyV2 {
    SigningKeyV2::from_dev_id(id)
}

fn policy_signatures(policy: &[u8], signers: &[&str]) -> Vec<u8> {
    let policy_sha256 = hex::encode(Sha256::digest(policy));
    let payload = serde_json::json!({
        "context": OPERATIONAL_RELEASE_TRUST_POLICY_SIGNING_CONTEXT,
        "policySha256": policy_sha256
    });
    canonicalize(&serde_json::json!({
        "schema": "boole.operational-release-trust-policy-signatures.v1",
        "policySha256": policy_sha256,
        "signatures": signers.iter().map(|id| {
            let key = policy_key(id);
            let envelope = key.sign(&payload).expect("policy KAT signature");
            serde_json::json!({
                "keyId": id,
                "publicKey": envelope.pk,
                "signature": envelope.signature
            })
        }).collect::<Vec<_>>()
    }))
}

fn write_initial_policy_files(
    fixture: &FixtureDir,
    roots: &BootableCurlProductKatRoots,
) -> (PathBuf, PathBuf, PathBuf, String) {
    let recovery_root = canonicalize(&serde_json::json!({
        "schema": "boole.operational-release-recovery-root.v1",
        "threshold": 2,
        "keys": ([POLICY_RECOVERY_A, POLICY_RECOVERY_B, POLICY_RECOVERY_C]
            .iter()
            .map(|id| serde_json::json!({
                "keyId": id,
                "publicKey": policy_key(id).pk_hex()
            }))
            .collect::<Vec<_>>())
    }));
    let policy = canonicalize(&serde_json::json!({
        "schema": "boole.operational-release-trust-policy.v1",
        "generation": 1,
        "previousPolicySha256": null,
        "productRelease": {
            "status": "active",
            "keyId": roots.product_key_id,
            "publicKey": roots.product_public_key_hex
        },
        "guestRelease": {
            "status": "active",
            "keyId": roots.guest_key_id,
            "publicKey": roots.guest_public_key_hex
        },
        "recovery": {
            "threshold": 2,
            "keys": ([POLICY_RECOVERY_A, POLICY_RECOVERY_B, POLICY_RECOVERY_C]
                .iter()
                .map(|id| serde_json::json!({
                    "keyId": id,
                    "publicKey": policy_key(id).pk_hex()
                }))
                .collect::<Vec<_>>())
        },
        "retiredKeys": []
    }));
    let policy_sha256 = hex::encode(Sha256::digest(&policy));
    let recovery_path = fixture.join("installed-policy-recovery-root.json");
    let policy_path = fixture.join("installed-policy-v1.json");
    let signatures_path = fixture.join("installed-policy-v1-signatures.json");
    fs::write(&recovery_path, recovery_root).expect("recovery root");
    fs::write(&policy_path, &policy).expect("initial policy");
    fs::write(
        &signatures_path,
        policy_signatures(&policy, &[POLICY_RECOVERY_A, POLICY_RECOVERY_B]),
    )
    .expect("initial signatures");
    (recovery_path, policy_path, signatures_path, policy_sha256)
}

fn write_successor_policy_files(
    fixture: &FixtureDir,
    roots: &BootableCurlProductKatRoots,
    previous_policy_sha256: &str,
) -> (PathBuf, PathBuf, String) {
    let policy = canonicalize(&serde_json::json!({
        "schema": "boole.operational-release-trust-policy.v1",
        "generation": 2,
        "previousPolicySha256": previous_policy_sha256,
        "productRelease": {
            "status": "active",
            "keyId": roots.product_key_id,
            "publicKey": roots.product_public_key_hex
        },
        "guestRelease": {
            "status": "active",
            "keyId": roots.guest_key_id,
            "publicKey": roots.guest_public_key_hex
        },
        "recovery": {
            "threshold": 2,
            "keys": ([POLICY_RECOVERY_B, POLICY_RECOVERY_C, POLICY_RECOVERY_D]
                .iter()
                .map(|id| serde_json::json!({
                    "keyId": id,
                    "publicKey": policy_key(id).pk_hex()
                }))
                .collect::<Vec<_>>())
        },
        "retiredKeys": [{
            "role": "recovery",
            "keyId": POLICY_RECOVERY_A,
            "publicKey": policy_key(POLICY_RECOVERY_A).pk_hex(),
            "retiredAtGeneration": 2
        }]
    }));
    let policy_sha256 = hex::encode(Sha256::digest(&policy));
    let policy_path = fixture.join("installed-policy-v2.json");
    let signatures_path = fixture.join("installed-policy-v2-signatures.json");
    fs::write(&policy_path, &policy).expect("successor policy");
    fs::write(
        &signatures_path,
        policy_signatures(
            &policy,
            &[POLICY_RECOVERY_A, POLICY_RECOVERY_B, POLICY_RECOVERY_C],
        ),
    )
    .expect("successor signatures");
    (policy_path, signatures_path, policy_sha256)
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

#[allow(clippy::too_many_arguments)]
fn policy_install_args(
    base_url: &str,
    install_root: &Path,
    staging: &Path,
    recovery_root: Option<&Path>,
    policy: Option<&Path>,
    signatures: Option<&Path>,
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
    for (flag, path) in [
        ("--recovery-root", recovery_root),
        ("--trust-policy", policy),
        ("--trust-policy-signatures", signatures),
    ] {
        if let Some(path) = path {
            args.push(flag.into());
            args.push(path.display().to_string());
        }
    }
    args
}

#[test]
fn policy_backed_install_update_inspect_and_rollback_share_one_durable_chain() {
    let fixture = FixtureDir::new();
    let first_release = BootableCurlProductKatRelease::default();
    let (first_bundle, roots) = build_bundle(&fixture, "policy-first", first_release);
    let first_product_digest = digest(&first_bundle.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE));
    let first_guest_digest = digest(&first_bundle.join("guest-update-manifest"));
    let (recovery_root, first_policy, first_signatures, first_policy_sha256) =
        write_initial_policy_files(&fixture, &roots);

    let install_root = fixture.join("policy-install");
    let staging = fixture.join("policy-download-staging");
    let first_server = StaticLoopbackServer::start(&first_bundle);
    let (first, first_output) = run_cli(&policy_install_args(
        &first_server.base_url(),
        &install_root,
        &staging,
        Some(&recovery_root),
        Some(&first_policy),
        Some(&first_signatures),
    ));
    assert!(
        first_output.status.success(),
        "policy first install: {first}"
    );
    assert_eq!(first["result"]["trustPolicyGeneration"], 1);
    assert_eq!(first["result"]["trustPolicySha256"], first_policy_sha256);
    assert!(install_root
        .join(OPERATIONAL_RELEASE_TRUST_STATE_FILE)
        .is_file());
    drop(first_server);

    let (second_policy, second_signatures, second_policy_sha256) =
        write_successor_policy_files(&fixture, &roots, &first_policy_sha256);
    let (policy_update, policy_update_output) = run_cli(&[
        "product".into(),
        "update-trust-policy".into(),
        "--install-root".into(),
        install_root.display().to_string(),
        "--trust-policy".into(),
        second_policy.display().to_string(),
        "--trust-policy-signatures".into(),
        second_signatures.display().to_string(),
    ]);
    assert!(
        policy_update_output.status.success(),
        "policy successor: {policy_update}"
    );
    assert_eq!(policy_update["result"]["generation"], 2);
    assert_eq!(
        policy_update["result"]["policySha256"],
        second_policy_sha256
    );
    assert_eq!(
        policy_update["result"]["previousPolicySha256"],
        first_policy_sha256
    );

    let policy_state_path = install_root.join(OPERATIONAL_RELEASE_TRUST_STATE_FILE);
    let policy_state_before_replay = fs::read(&policy_state_path).expect("policy state");
    let (policy_replay, policy_replay_output) = run_cli(&[
        "product".into(),
        "update-trust-policy".into(),
        "--install-root".into(),
        install_root.display().to_string(),
        "--trust-policy".into(),
        second_policy.display().to_string(),
        "--trust-policy-signatures".into(),
        second_signatures.display().to_string(),
    ]);
    assert!(
        !policy_replay_output.status.success(),
        "policy replay was accepted: {policy_replay}"
    );
    assert_eq!(policy_replay["error"]["reason"], "trust-policy-rejected");
    assert_eq!(
        fs::read(&policy_state_path).expect("policy state after replay"),
        policy_state_before_replay,
        "a rejected policy replay must not change the durable chain"
    );

    let (direct_reopen, direct_reopen_output) = run_cli(&lifecycle_args(
        "inspect-direct-boot",
        &install_root,
        &roots,
    ));
    assert!(
        !direct_reopen_output.status.success(),
        "direct roots reopened a policy-backed install: {direct_reopen}"
    );
    assert_eq!(direct_reopen["error"]["reason"], "trust-policy-rejected");
    assert_eq!(
        fs::read(&policy_state_path).expect("policy state after direct-root attempt"),
        policy_state_before_replay,
        "the development compatibility mode must not mutate policy state"
    );

    let (inspect, inspect_output) = run_cli(&[
        "product".into(),
        "inspect-direct-boot".into(),
        "--install-root".into(),
        install_root.display().to_string(),
    ]);
    assert!(inspect_output.status.success(), "policy inspect: {inspect}");
    assert_eq!(inspect["result"]["trustPolicy"]["generation"], 2);
    assert_eq!(
        inspect["result"]["trustPolicy"]["previousPolicySha256"],
        first_policy_sha256
    );
    assert_eq!(inspect["result"]["trustPolicy"]["policyDirectoryCount"], 2);
    assert_eq!(
        inspect["result"]["trustPolicy"]["unreferencedPolicyDirectoryCount"],
        0
    );

    let second_release = BootableCurlProductKatRelease {
        product_sequence: 2,
        product_version: "0.0.1-policy-chain-kat".to_string(),
        product_previous_manifest_sha256: Some(first_product_digest),
        guest_sequence: 2,
        guest_version: "0.0.1-policy-chain-kat".to_string(),
        guest_previous_manifest_sha256: Some(first_guest_digest),
    };
    let (second_bundle, second_roots) = build_bundle(&fixture, "policy-second", second_release);
    assert_eq!(roots, second_roots);
    let second_server = StaticLoopbackServer::start(&second_bundle);
    let (second, second_output) = run_cli(&policy_install_args(
        &second_server.base_url(),
        &install_root,
        &staging,
        None,
        None,
        None,
    ));
    assert!(
        second_output.status.success(),
        "policy update install: {second}"
    );
    assert_eq!(second["result"]["releaseSequence"], 2);
    assert_eq!(second["result"]["trustPolicyGeneration"], 2);
    drop(second_server);

    let (rollback, rollback_output) = run_cli(&[
        "product".into(),
        "rollback-direct-boot".into(),
        "--install-root".into(),
        install_root.display().to_string(),
    ]);
    assert!(
        rollback_output.status.success(),
        "policy rollback: {rollback}"
    );
    assert_eq!(rollback["result"]["activeReleaseSequence"], 1);
    assert_eq!(rollback["result"]["trustPolicyGeneration"], 2);

    let installed_policy =
        boole_core::open_installed_operational_release_trust_policy(&install_root)
            .expect("installed policy before tamper");
    fs::set_permissions(
        installed_policy.policy_path(),
        fs::Permissions::from_mode(0o644),
    )
    .expect("make installed policy writable for attacker simulation");
    fs::write(installed_policy.policy_path(), b"{}\n").expect("tamper installed policy");
    let state_root = fixture.join("run-state-must-stay-absent");
    let (run, run_output) = run_cli(&[
        "product".into(),
        "run-direct-boot".into(),
        "--install-root".into(),
        install_root.display().to_string(),
        "--state-root".into(),
        state_root.display().to_string(),
    ]);
    assert!(!run_output.status.success(), "tampered policy ran: {run}");
    assert_eq!(run["error"]["reason"], "trust-policy-rejected");
    assert!(
        !state_root.exists(),
        "runtime started before policy verification"
    );
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

    let state_before_inspect =
        fs::read(install_root.join(CURL_PRODUCT_INSTALL_STATE_FILE)).expect("state before inspect");
    let (inspect, inspect_output) = run_cli(&lifecycle_args(
        "inspect-direct-boot",
        &install_root,
        &roots,
    ));
    assert!(inspect_output.status.success(), "inspect: {inspect}");
    assert_eq!(inspect["command"], "product.inspect-direct-boot");
    assert_eq!(inspect["result"]["activeRelease"]["releaseSequence"], 1);
    assert_eq!(
        inspect["result"]["activeRelease"]["guestReleaseSequence"],
        1
    );
    assert_eq!(
        inspect["result"]["securityFloors"]["productReleaseSequence"],
        2
    );
    assert_eq!(
        inspect["result"]["securityFloors"]["guestReleaseSequence"],
        2
    );
    assert_eq!(inspect["result"]["rollbackRelease"]["releaseSequence"], 2);
    assert_eq!(
        inspect["result"]["rollbackRelease"]["guestReleaseSequence"],
        2
    );
    assert_eq!(inspect["result"]["storage"]["versionDirectoryCount"], 2);
    assert_eq!(
        inspect["result"]["storage"]["unreferencedVersionDirectoryCount"],
        0
    );
    assert_eq!(inspect["result"]["storage"]["unexpectedEntryCount"], 0);
    assert_eq!(inspect["result"]["storage"]["clean"], true);
    assert_eq!(
        fs::read(install_root.join(CURL_PRODUCT_INSTALL_STATE_FILE))
            .expect("state after read-only inspect"),
        state_before_inspect,
        "inspection must not mutate durable install state"
    );

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

    let corrupt_state_before_inspect =
        fs::read(install_root.join(CURL_PRODUCT_INSTALL_STATE_FILE)).expect("corrupt state");
    let (rejected_inspect, rejected_inspect_output) = run_cli(&lifecycle_args(
        "inspect-direct-boot",
        &install_root,
        &roots,
    ));
    assert!(
        !rejected_inspect_output.status.success(),
        "corrupt release inspected as healthy: {rejected_inspect}"
    );
    assert_eq!(
        rejected_inspect["error"]["reason"],
        "installed-release-rejected"
    );
    assert_eq!(
        fs::read(install_root.join(CURL_PRODUCT_INSTALL_STATE_FILE))
            .expect("state after rejected inspect"),
        corrupt_state_before_inspect,
        "inspection must not repair or replace a corrupt release"
    );

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

#[test]
fn interrupted_update_converges_before_and_after_the_state_swap() {
    let fixture = FixtureDir::new();
    let first_release = BootableCurlProductKatRelease::default();
    let (first_bundle, roots) = build_bundle(&fixture, "crash-first", first_release);
    let first_product_digest = digest(&first_bundle.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE));
    let first_guest_digest = digest(&first_bundle.join("guest-update-manifest"));
    let second_release = BootableCurlProductKatRelease {
        product_sequence: 2,
        product_version: "0.0.1-installed-mac-crash-kat".to_string(),
        product_previous_manifest_sha256: Some(first_product_digest),
        guest_sequence: 2,
        guest_version: "0.0.1-installed-mac-crash-kat".to_string(),
        guest_previous_manifest_sha256: Some(first_guest_digest),
    };
    let (second_bundle, second_roots) = build_bundle(&fixture, "crash-second", second_release);
    assert_eq!(roots, second_roots);
    let second_product_digest = digest(&second_bundle.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE));
    let second_guest_digest = digest(&second_bundle.join("guest-update-manifest"));
    let third_release = BootableCurlProductKatRelease {
        product_sequence: 3,
        product_version: "0.0.2-installed-mac-crash-kat".to_string(),
        product_previous_manifest_sha256: Some(second_product_digest),
        guest_sequence: 3,
        guest_version: "0.0.2-installed-mac-crash-kat".to_string(),
        guest_previous_manifest_sha256: Some(second_guest_digest),
    };
    let (third_bundle, third_roots) = build_bundle(&fixture, "crash-third", third_release);
    assert_eq!(roots, third_roots);

    let install_root = fixture.join("crash-install");
    let staging = fixture.join("crash-download-staging");
    let first_server = StaticLoopbackServer::start(&first_bundle);
    let (first, first_output) = run_cli(&install_args(
        &first_server.base_url(),
        &install_root,
        &staging,
        &roots,
    ));
    assert!(first_output.status.success(), "first install: {first}");
    drop(first_server);

    let first_state = read_installed_curl_product_state(&install_root)
        .expect("read first state")
        .expect("first state exists");
    let first_version_directory = first_state.version_directory().to_string();
    let state_path = install_root.join(CURL_PRODUCT_INSTALL_STATE_FILE);
    let state_before_precommit_failure = fs::read(&state_path).expect("first durable state");

    // A directory at the fixed temporary-state path forces the installer to
    // fail after adopting the complete version directory but before the
    // atomic state-file replacement. The prior state must remain exact.
    let state_temp_blocker = install_root.join("installed-release.json.tmp");
    fs::create_dir(&state_temp_blocker).expect("state temp blocker");
    let second_server = StaticLoopbackServer::start(&second_bundle);
    let (precommit_failure, precommit_output) = run_cli(&install_args(
        &second_server.base_url(),
        &install_root,
        &staging,
        &roots,
    ));
    assert!(
        !precommit_output.status.success(),
        "pre-commit failure was accepted: {precommit_failure}"
    );
    assert_eq!(precommit_failure["error"]["reason"], "install-rejected");
    assert_eq!(
        fs::read(&state_path).expect("state after pre-commit failure"),
        state_before_precommit_failure,
        "a failure before state replacement must preserve the prior state byte-for-byte"
    );
    let second_version_directory = format!(
        "{:012}-{}",
        2,
        &digest(&second_bundle.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE))[..12]
    );
    assert!(
        install_root
            .join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY)
            .join(&second_version_directory)
            .is_dir(),
        "the test must reach the adopted-version/pre-state window"
    );
    drop(second_server);
    fs::remove_dir(&state_temp_blocker).expect("remove state temp blocker");

    let second_server = StaticLoopbackServer::start(&second_bundle);
    let (second, second_output) = run_cli(&install_args(
        &second_server.base_url(),
        &install_root,
        &staging,
        &roots,
    ));
    assert!(
        second_output.status.success(),
        "retry second install: {second}"
    );
    drop(second_server);

    // Keep the child alive after the state swap by giving post-commit pruning
    // a large, contract-shaped but unreferenced directory to remove. The
    // parent observes sequence three in the atomic state file, then sends a
    // real SIGKILL to the actual CLI process.
    let junk = install_root
        .join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY)
        .join("000000000999-aaaaaaaaaaaa");
    fs::create_dir(&junk).expect("post-commit pruning fixture");
    for index in 0..50_000_u32 {
        fs::write(junk.join(format!("residue-{index:05}")), []).expect("pruning residue");
    }
    let third_server = StaticLoopbackServer::start(&third_bundle);
    let third_args = install_args(&third_server.base_url(), &install_root, &staging, &roots);
    let mut child = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(&third_args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn third install");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let sequence = read_installed_curl_product_state(&install_root)
            .ok()
            .flatten()
            .map(|state| state.release_sequence());
        if sequence == Some(3) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "third state was not adopted in time"
        );
        assert!(
            child.try_wait().expect("poll third install").is_none(),
            "third install exited before its committed state could be interrupted"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    child
        .kill()
        .expect("SIGKILL the exact spawned install child");
    let killed = child.wait().expect("reap killed install");
    assert_eq!(killed.signal(), Some(libc::SIGKILL));
    drop(third_server);

    let committed = read_installed_curl_product_state(&install_root)
        .expect("read committed state after kill")
        .expect("committed state after kill");
    assert_eq!(committed.release_sequence(), 3);
    assert_eq!(committed.release_floor_sequence(), 3);
    assert_eq!(committed.guest_release_floor_sequence(), Some(3));

    let (rollback, rollback_output) = run_cli(&lifecycle_args(
        "rollback-direct-boot",
        &install_root,
        &roots,
    ));
    assert!(
        rollback_output.status.success(),
        "rollback after kill: {rollback}"
    );
    assert_eq!(rollback["result"]["activeReleaseSequence"], 2);
    assert_eq!(rollback["result"]["releaseFloorSequence"], 3);
    assert_eq!(rollback["result"]["guestReleaseFloorSequence"], 3);
    assert!(
        !junk.exists(),
        "the next verified mutation must reconcile post-commit crash residue"
    );
    assert!(
        !install_root
            .join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY)
            .join(first_version_directory)
            .exists(),
        "the next verified mutation must prune the now-unreferenced first generation"
    );
}

#[test]
fn an_active_product_mutation_lease_rejects_a_competing_cli_before_network_io() {
    let fixture = FixtureDir::new();
    let first_release = BootableCurlProductKatRelease::default();
    let (first_bundle, roots) = build_bundle(&fixture, "lease-first", first_release);
    let second_release = BootableCurlProductKatRelease {
        product_sequence: 2,
        product_version: "0.0.1-installed-mac-lease-kat".to_string(),
        product_previous_manifest_sha256: Some(digest(
            &first_bundle.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE),
        )),
        guest_sequence: 2,
        guest_version: "0.0.1-installed-mac-lease-kat".to_string(),
        guest_previous_manifest_sha256: Some(digest(&first_bundle.join("guest-update-manifest"))),
    };
    let (second_bundle, second_roots) = build_bundle(&fixture, "lease-second", second_release);
    assert_eq!(roots, second_roots);

    let install_root = fixture.join("lease-install");
    let staging = fixture.join("shared-download-staging");
    let first_server = StaticLoopbackServer::start(&first_bundle);
    let (first, first_output) = run_cli(&install_args(
        &first_server.base_url(),
        &install_root,
        &staging,
        &roots,
    ));
    assert!(first_output.status.success(), "first install: {first}");
    drop(first_server);
    let state_before_competitor =
        fs::read(install_root.join(CURL_PRODUCT_INSTALL_STATE_FILE)).expect("state before lease");

    let lease = acquire_installed_product_mutation_lease(&install_root)
        .expect("test process acquires mutation lease");
    let second_server = StaticLoopbackServer::start(&second_bundle);
    let (busy, busy_output) = run_cli(&install_args(
        &second_server.base_url(),
        &install_root,
        &staging,
        &roots,
    ));
    assert!(!busy_output.status.success(), "competing install: {busy}");
    assert_eq!(busy["error"]["reason"], "product-busy");
    assert!(
        second_server.requests().is_empty(),
        "the losing command must not make even a manifest request"
    );
    assert_eq!(
        fs::read(install_root.join(CURL_PRODUCT_INSTALL_STATE_FILE))
            .expect("state after rejected competitor"),
        state_before_competitor
    );
    assert!(!staging.exists(), "the losing command creates no staging");

    drop(lease);
    fs::create_dir(&staging).expect("caller-owned staging anchor");
    fs::write(staging.join("caller-owned"), b"do not clear this directory")
        .expect("caller-owned staging sentinel");
    let (second, second_output) = run_cli(&install_args(
        &second_server.base_url(),
        &install_root,
        &staging,
        &roots,
    ));
    assert!(
        second_output.status.success(),
        "install after lease release: {second}"
    );
    assert_eq!(second["result"]["releaseSequence"], 2);
    assert_eq!(second_server.requests().len(), 19);
    assert_eq!(
        fs::read(staging.join("caller-owned")).expect("staging sentinel survives"),
        b"do not clear this directory"
    );
    let attempt_prefix = format!(
        "{}.boole-attempt-",
        staging.file_name().expect("staging name").to_string_lossy()
    );
    assert!(
        fs::read_dir(staging.parent().expect("staging parent"))
            .expect("read staging parent")
            .all(|entry| !entry
                .expect("staging sibling")
                .file_name()
                .to_string_lossy()
                .starts_with(&attempt_prefix)),
        "the successful command removes its unique attempt staging"
    );
}
