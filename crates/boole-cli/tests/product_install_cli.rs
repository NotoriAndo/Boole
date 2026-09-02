//! CURL.2-TRANSPORT — `boole product install` CLI entrypoint tests.
//!
//! Closed-local only: a loopback HTTP server and a non-production KAT key.
//! The CLI is the curl entrypoint that drives the verified transport and
//! the CURL.2-CORE installer; these tests pin its envelope surface and its
//! fail-closed exit behavior.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::{
    canonicalize, GuestArtifactRole, ProductArtifactRole, SigningKeyV2,
    CURL_PRODUCT_INSTALLED_MANIFEST_FILE, CURL_PRODUCT_INSTALLED_SIGNATURE_FILE,
    CURL_PRODUCT_INSTALL_STATE_FILE, CURL_PRODUCT_RELEASE_SIGNING_CONTEXT,
    NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
};
use serde_json::json;
use sha2::{Digest, Sha256};

const KAT_KEY_ID: &str = "non-production-kat-2026-08-25";
const KAT_SOURCE_REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BUNDLE_PATH: &str = "/bundle";

fn guest_manifest_bytes() -> Vec<u8> {
    let artifacts: BTreeMap<GuestArtifactRole, Vec<u8>> = GuestArtifactRole::ALL
        .into_iter()
        .map(|role| {
            (
                role,
                format!("guest-fixture:{}", role.as_str()).into_bytes(),
            )
        })
        .collect();
    let descriptors: Vec<_> = GuestArtifactRole::ALL
        .into_iter()
        .map(|role| {
            let bytes = &artifacts[&role];
            json!({
                "role": role.as_str(),
                "fileName": format!("{}.bin", role.as_str()),
                "byteLength": bytes.len(),
                "sha256": hex::encode(Sha256::digest(bytes)),
            })
        })
        .collect();
    canonicalize(&json!({
        "schema": "boole.native-shadow.guest-update-manifest.v1",
        "channel": "stable",
        "releaseSequence": 1,
        "releaseVersion": "1.0.0",
        "targetOs": "linux",
        "targetArch": "aarch64",
        "previousManifestSha256": null,
        "artifacts": descriptors,
    }))
}

fn envelope_for(
    signing_key: &SigningKeyV2,
    key_id: &str,
    manifest: &[u8],
    context: &str,
) -> Vec<u8> {
    let payload: serde_json::Value = serde_json::from_slice(manifest).expect("manifest payload");
    let envelope = signing_key
        .sign_for_network(&payload, Some(context))
        .expect("KAT signature");
    canonicalize(&json!({
        "schema": envelope.schema,
        "keyId": key_id,
        "pk": envelope.pk,
        "signature": envelope.signature,
        "networkId": envelope.network_id,
        "manifestSha256": hex::encode(Sha256::digest(manifest)),
    }))
}

struct BundleFixture {
    public_key_hex: String,
    manifest: Vec<u8>,
    signature: Vec<u8>,
    artifacts: BTreeMap<ProductArtifactRole, Vec<u8>>,
}

fn signed_bundle_fixture() -> BundleFixture {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let guest_manifest = guest_manifest_bytes();
    let guest_signature = envelope_for(
        &signing_key,
        KAT_KEY_ID,
        &guest_manifest,
        NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
    );
    let mut artifacts: BTreeMap<ProductArtifactRole, Vec<u8>> = [
        ProductArtifactRole::HostCli,
        ProductArtifactRole::HostNode,
        ProductArtifactRole::HostWalletAgent,
        ProductArtifactRole::HostController,
    ]
    .into_iter()
    .map(|role| (role, format!("host-fixture:{}", role.as_str()).into_bytes()))
    .collect();
    artifacts.insert(ProductArtifactRole::GuestUpdateManifest, guest_manifest);
    artifacts.insert(ProductArtifactRole::GuestUpdateSignature, guest_signature);

    let descriptors: Vec<_> = ProductArtifactRole::ALL
        .into_iter()
        .map(|role| {
            let bytes = &artifacts[&role];
            json!({
                "role": role.as_str(),
                "fileName": format!("{}.bin", role.as_str()),
                "byteLength": bytes.len(),
                "sha256": hex::encode(Sha256::digest(bytes)),
            })
        })
        .collect();
    let guest_manifest_sha256 = hex::encode(Sha256::digest(
        &artifacts[&ProductArtifactRole::GuestUpdateManifest],
    ));
    let manifest = canonicalize(&json!({
        "schema": "boole.curl-product-release.v1",
        "channel": "stable",
        "releaseSequence": 1,
        "releaseVersion": "1.0.0",
        "sourceRevision": KAT_SOURCE_REVISION,
        "targetOs": "macos",
        "targetArch": "arm64",
        "minimumMacOs": "14.0",
        "previousManifestSha256": null,
        "controllerProtocolVersion": 1,
        "guestManifestSha256": guest_manifest_sha256,
        "guestReleaseSequence": 1,
        "guestReleaseVersion": "1.0.0",
        "artifacts": descriptors,
    }));
    let signature = envelope_for(
        &signing_key,
        KAT_KEY_ID,
        &manifest,
        CURL_PRODUCT_RELEASE_SIGNING_CONTEXT,
    );
    BundleFixture {
        public_key_hex: signing_key.pk_hex(),
        manifest,
        signature,
        artifacts,
    }
}

struct FixtureDir {
    root: PathBuf,
}

impl FixtureDir {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "boole-product-install-cli-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create fixture dir");
        Self { root }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct LoopbackServer {
    port: u16,
    routes: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl LoopbackServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
        let port = listener.local_addr().expect("loopback address").port();
        let routes: Arc<Mutex<BTreeMap<String, Vec<u8>>>> = Arc::default();
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_routes = Arc::clone(&routes);
        let thread_shutdown = Arc::clone(&shutdown);
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if thread_shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { break };
                serve_connection(stream, &thread_routes);
            }
        });
        Self {
            port,
            routes,
            shutdown,
            handle: Some(handle),
        }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}{BUNDLE_PATH}", self.port)
    }

    fn set_route(&self, path: &str, body: Vec<u8>) {
        self.routes
            .lock()
            .expect("routes lock")
            .insert(path.to_string(), body);
    }
}

impl Drop for LoopbackServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn serve_connection(mut stream: TcpStream, routes: &Arc<Mutex<BTreeMap<String, Vec<u8>>>>) {
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !raw.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => raw.extend_from_slice(&chunk[..read]),
            Err(_) => return,
        }
    }
    let head = String::from_utf8_lossy(&raw);
    let Some(request_line) = head.lines().next() else {
        return;
    };
    let Some(path) = request_line.split_whitespace().nth(1) else {
        return;
    };
    let route = routes.lock().expect("routes lock").get(path).cloned();
    let (status, reason, body) = match route {
        Some(body) => (200, "OK", body),
        None => (404, "Not Found", Vec::new()),
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

fn serve_bundle(server: &LoopbackServer, fixture: &BundleFixture) {
    server.set_route(
        &format!("{BUNDLE_PATH}/{CURL_PRODUCT_INSTALLED_MANIFEST_FILE}"),
        fixture.manifest.clone(),
    );
    server.set_route(
        &format!("{BUNDLE_PATH}/{CURL_PRODUCT_INSTALLED_SIGNATURE_FILE}"),
        fixture.signature.clone(),
    );
    for role in ProductArtifactRole::ALL {
        server.set_route(
            &format!("{BUNDLE_PATH}/{}.bin", role.as_str()),
            fixture.artifacts[&role].clone(),
        );
    }
}

fn run_product_install(
    base_url: &str,
    root: &std::path::Path,
    staging: &std::path::Path,
    public_key_hex: &str,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "product",
            "install",
            "--base-url",
            base_url,
            "--install-root",
            &root.display().to_string(),
            "--download-staging",
            &staging.display().to_string(),
            "--trust-root-key-id",
            KAT_KEY_ID,
            "--trust-root-public-key",
            public_key_hex,
            "--first-install-minimum",
            "1",
        ])
        .output()
        .expect("spawn boole-cli")
}

#[test]
fn product_install_downloads_verifies_and_installs_via_the_cli() {
    let fixture = signed_bundle_fixture();
    let server = LoopbackServer::start();
    serve_bundle(&server, &fixture);
    let dir = FixtureDir::new("happy");
    let root = dir.path("root");
    let staging = dir.path("download-staging");

    let output = run_product_install(&server.base_url(), &root, &staging, &fixture.public_key_hex);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains("\"ok\":true"), "stdout: {stdout}");
    assert!(
        stdout.contains("\"command\":\"product.install\""),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"releaseVersion\":\"1.0.0\""),
        "stdout: {stdout}"
    );
    assert!(root.join(CURL_PRODUCT_INSTALL_STATE_FILE).exists());
    assert!(!staging.exists());
}

#[test]
fn product_install_reports_a_rejected_release_and_exits_nonzero() {
    let fixture = signed_bundle_fixture();
    let attacker = SigningKeyV2::from_dev_id("attacker-not-the-trust-root");
    let forged_signature = envelope_for(
        &attacker,
        KAT_KEY_ID,
        &fixture.manifest,
        CURL_PRODUCT_RELEASE_SIGNING_CONTEXT,
    );
    let server = LoopbackServer::start();
    serve_bundle(&server, &fixture);
    server.set_route(
        &format!("{BUNDLE_PATH}/{CURL_PRODUCT_INSTALLED_SIGNATURE_FILE}"),
        forged_signature,
    );
    let dir = FixtureDir::new("forged");
    let root = dir.path("root");
    let staging = dir.path("download-staging");

    let output = run_product_install(&server.base_url(), &root, &staging, &fixture.public_key_hex);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("\"ok\":false"), "stderr: {stderr}");
    assert!(
        stderr.contains("\"reason\":\"release-rejected\""),
        "stderr: {stderr}"
    );
    assert!(!root.join(CURL_PRODUCT_INSTALL_STATE_FILE).exists());
    assert!(!staging.exists());
}

#[test]
fn product_install_bootable_exposes_both_explicit_trust_roots() {
    let dir = FixtureDir::new("bootable-cli-contract");
    let root = dir.path("root");
    let staging = dir.path("download-staging");
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "product",
            "install-bootable",
            "--base-url",
            "file:///untrusted",
            "--install-root",
            &root.display().to_string(),
            "--download-staging",
            &staging.display().to_string(),
            "--product-trust-root-key-id",
            KAT_KEY_ID,
            "--product-trust-root-public-key",
            &"11".repeat(32),
            "--guest-trust-root-key-id",
            "guest-kat",
            "--guest-trust-root-public-key",
            &"22".repeat(32),
            "--first-product-minimum",
            "1",
            "--first-guest-minimum",
            "1",
        ])
        .output()
        .expect("spawn boole-cli");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr: {stderr}");
    assert!(
        stderr.contains("\"command\":\"product.install-bootable\""),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("\"reason\":\"url-rejected\""),
        "stderr: {stderr}"
    );
}
