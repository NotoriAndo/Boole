//! CURL.2-TRANSPORT closed-local integration tests.
//!
//! Every test runs against a loopback HTTP server started inside the test
//! process and a non-production KAT signing key. No public network, no
//! production trust root and no real release artifact is involved. The
//! tests pin the transport invariants: downloads land only in a transient
//! staging directory, nothing is fetched past a failed verification step,
//! and URL/HTTP status/file names never act as trust grounds.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use boole_cli::curl_product_transport::{
    download_and_install_bootable_curl_product_release, download_and_install_curl_product_release,
    CurlProductTransportError,
};
use boole_core::{
    canonicalize, read_installed_curl_product_state, CurlProductInstallError,
    CurlProductReleaseTrustRoot, CurlProductReleaseVerifyError, GuestArtifactRole,
    InstalledCurlProduct, NativeShadowUpdateTrustRoot, ProductArtifactRole, SigningKeyV2,
    CURL_PRODUCT_INSTALLED_MANIFEST_FILE, CURL_PRODUCT_INSTALLED_SIGNATURE_FILE,
    CURL_PRODUCT_INSTALL_STAGING_DIRECTORY, CURL_PRODUCT_INSTALL_STATE_FILE,
    CURL_PRODUCT_INSTALL_STATE_TEMP_FILE, CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY,
    CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V2, CURL_PRODUCT_RELEASE_SIGNING_CONTEXT,
    CURL_PRODUCT_RELEASE_SIGNING_CONTEXT_V2, GUEST_UPDATE_MANIFEST_SCHEMA_V2,
    MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES, NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
    NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V2,
};
use serde_json::json;
use sha2::{Digest, Sha256};

const KAT_KEY_ID: &str = "non-production-kat-2026-08-25";
const KAT_SOURCE_REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BUNDLE_PATH: &str = "/bundle";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn guest_artifact_bytes() -> BTreeMap<GuestArtifactRole, Vec<u8>> {
    GuestArtifactRole::ALL
        .into_iter()
        .map(|role| {
            (
                role,
                format!("guest-fixture:{}", role.as_str()).into_bytes(),
            )
        })
        .collect()
}

fn guest_manifest_bytes() -> Vec<u8> {
    let artifacts = guest_artifact_bytes();
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

fn product_artifact_bytes() -> BTreeMap<ProductArtifactRole, Vec<u8>> {
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
    artifacts
}

fn product_manifest_bytes(artifacts: &BTreeMap<ProductArtifactRole, Vec<u8>>) -> Vec<u8> {
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
    canonicalize(&json!({
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
    }))
}

fn product_signature_for(signing_key: &SigningKeyV2, key_id: &str, manifest: &[u8]) -> Vec<u8> {
    envelope_for(
        signing_key,
        key_id,
        manifest,
        CURL_PRODUCT_RELEASE_SIGNING_CONTEXT,
    )
}

type ProductFixture = (
    CurlProductReleaseTrustRoot,
    Vec<u8>,
    Vec<u8>,
    BTreeMap<ProductArtifactRole, Vec<u8>>,
);

fn signed_product_fixture() -> ProductFixture {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let artifacts = product_artifact_bytes();
    let manifest = product_manifest_bytes(&artifacts);
    let signature = product_signature_for(&signing_key, KAT_KEY_ID, &manifest);
    let trust_root = CurlProductReleaseTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    (trust_root, manifest, signature, artifacts)
}

fn signed_successor_fixture(previous_manifest_sha256: &str) -> ProductFixture {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let artifacts = product_artifact_bytes();
    let base = product_manifest_bytes(&artifacts);
    let mut value: serde_json::Value = serde_json::from_slice(&base).expect("manifest JSON");
    value["releaseSequence"] = json!(2);
    value["releaseVersion"] = json!("1.0.1");
    value["previousManifestSha256"] = json!(previous_manifest_sha256);
    let manifest = canonicalize(&value);
    let signature = product_signature_for(&signing_key, KAT_KEY_ID, &manifest);
    let trust_root = CurlProductReleaseTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    (trust_root, manifest, signature, artifacts)
}

fn manifest_sha256_hex(fixture: &ProductFixture) -> String {
    hex::encode(Sha256::digest(&fixture.1))
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
            "boole-curl-product-transport-{label}-{}-{nonce}",
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

/// Route table: request path → (HTTP status, response body).
type RouteMap = BTreeMap<String, (u16, Vec<u8>)>;

/// Minimal single-threaded loopback HTTP/1.1 responder. Every response
/// closes its connection, so the client reconnects per request and the
/// request log observes one entry per fetch, in order.
struct LoopbackServer {
    port: u16,
    routes: Arc<Mutex<RouteMap>>,
    requests: Arc<Mutex<Vec<String>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl LoopbackServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
        let port = listener.local_addr().expect("loopback address").port();
        let routes: Arc<Mutex<RouteMap>> = Arc::default();
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_routes = Arc::clone(&routes);
        let thread_requests = Arc::clone(&requests);
        let thread_shutdown = Arc::clone(&shutdown);
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if thread_shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { break };
                serve_connection(stream, &thread_routes, &thread_requests);
            }
        });
        Self {
            port,
            routes,
            requests,
            shutdown,
            handle: Some(handle),
        }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}{BUNDLE_PATH}", self.port)
    }

    fn set_route(&self, path: &str, status: u16, body: Vec<u8>) {
        self.routes
            .lock()
            .expect("routes lock")
            .insert(path.to_string(), (status, body));
    }

    fn remove_route(&self, path: &str) {
        self.routes.lock().expect("routes lock").remove(path);
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests lock").clone()
    }

    fn clear_requests(&self) {
        self.requests.lock().expect("requests lock").clear();
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

fn serve_connection(
    mut stream: TcpStream,
    routes: &Arc<Mutex<RouteMap>>,
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
    let head = String::from_utf8_lossy(&raw);
    let Some(request_line) = head.lines().next() else {
        return;
    };
    let mut parts = request_line.split_whitespace();
    let _method = parts.next();
    let Some(path) = parts.next() else { return };
    requests
        .lock()
        .expect("requests lock")
        .push(path.to_string());
    let route = routes.lock().expect("routes lock").get(path).cloned();
    let (status, body) = route.unwrap_or((404, Vec::new()));
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    // The client may abort mid-body (declared-length and cap tests); a
    // broken pipe here is expected, never a server panic.
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

fn serve_bundle(server: &LoopbackServer, fixture: &ProductFixture) {
    server.set_route(
        &format!("{BUNDLE_PATH}/{CURL_PRODUCT_INSTALLED_MANIFEST_FILE}"),
        200,
        fixture.1.clone(),
    );
    server.set_route(
        &format!("{BUNDLE_PATH}/{CURL_PRODUCT_INSTALLED_SIGNATURE_FILE}"),
        200,
        fixture.2.clone(),
    );
    for role in ProductArtifactRole::ALL {
        server.set_route(
            &format!("{BUNDLE_PATH}/{}.bin", role.as_str()),
            200,
            fixture.3[&role].clone(),
        );
    }
}

fn artifact_route(role: ProductArtifactRole) -> String {
    format!("{BUNDLE_PATH}/{}.bin", role.as_str())
}

fn expected_bundle_requests() -> Vec<String> {
    let mut paths = vec![
        format!("{BUNDLE_PATH}/{CURL_PRODUCT_INSTALLED_MANIFEST_FILE}"),
        format!("{BUNDLE_PATH}/{CURL_PRODUCT_INSTALLED_SIGNATURE_FILE}"),
    ];
    paths.extend(ProductArtifactRole::ALL.into_iter().map(artifact_route));
    paths
}

fn download(
    base_url: &str,
    install_root: &Path,
    staging: &Path,
    fixture: &ProductFixture,
    first_install_minimum_sequence: u64,
) -> Result<InstalledCurlProduct, CurlProductTransportError> {
    download_and_install_curl_product_release(
        base_url,
        install_root,
        staging,
        &fixture.0,
        first_install_minimum_sequence,
        REQUEST_TIMEOUT,
    )
}

fn assert_root_untouched(root: &Path) {
    assert!(!root.join(CURL_PRODUCT_INSTALL_STATE_FILE).exists());
    assert!(!root.join(CURL_PRODUCT_INSTALL_STATE_TEMP_FILE).exists());
    assert!(!root.join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY).exists());
    assert!(!root.join(CURL_PRODUCT_INSTALL_STAGING_DIRECTORY).exists());
}

type BootableFixture = (
    CurlProductReleaseTrustRoot,
    NativeShadowUpdateTrustRoot,
    Vec<u8>,
    Vec<u8>,
    BTreeMap<ProductArtifactRole, Vec<u8>>,
    BTreeMap<GuestArtifactRole, Vec<u8>>,
);

fn signed_bootable_fixture() -> BootableFixture {
    signed_bootable_fixture_with_guest_signer(true)
}

fn signed_bootable_fixture_with_guest_signer(trusted: bool) -> BootableFixture {
    let product_key_id = "non-production-product-v2-transport-kat";
    let guest_key_id = "non-production-guest-v2-transport-kat";
    let product_key = SigningKeyV2::from_dev_id(product_key_id);
    let guest_key = SigningKeyV2::from_dev_id(guest_key_id);
    let guest_signer = if trusted {
        SigningKeyV2::from_dev_id(guest_key_id)
    } else {
        SigningKeyV2::from_dev_id("non-production-guest-v2-transport-attacker")
    };
    let guest_artifacts = GuestArtifactRole::BOOTABLE_ALL
        .into_iter()
        .map(|role| {
            (
                role,
                format!("bootable-transport-fixture:{}", role.as_str()).into_bytes(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let guest_descriptors = GuestArtifactRole::BOOTABLE_ALL
        .into_iter()
        .map(|role| {
            let bytes = &guest_artifacts[&role];
            json!({
                "role": role.as_str(),
                "fileName": format!("{}.bin", role.as_str()),
                "byteLength": bytes.len(),
                "sha256": hex::encode(Sha256::digest(bytes)),
            })
        })
        .collect::<Vec<_>>();
    let guest_manifest = canonicalize(&json!({
        "schema": GUEST_UPDATE_MANIFEST_SCHEMA_V2,
        "bootFormatVersion": 1,
        "channel": "stable",
        "releaseSequence": 1,
        "releaseVersion": "2.0.0",
        "targetOs": "linux",
        "targetArch": "aarch64",
        "previousManifestSha256": null,
        "artifacts": guest_descriptors,
    }));
    let guest_signature = envelope_for(
        &guest_signer,
        guest_key_id,
        &guest_manifest,
        NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V2,
    );
    let mut product_artifacts = [
        ProductArtifactRole::HostCli,
        ProductArtifactRole::HostNode,
        ProductArtifactRole::HostWalletAgent,
        ProductArtifactRole::HostController,
    ]
    .into_iter()
    .map(|role| {
        (
            role,
            format!("bootable-host-transport-fixture:{}", role.as_str()).into_bytes(),
        )
    })
    .collect::<BTreeMap<_, _>>();
    product_artifacts.insert(
        ProductArtifactRole::GuestUpdateManifest,
        guest_manifest.clone(),
    );
    product_artifacts.insert(ProductArtifactRole::GuestUpdateSignature, guest_signature);
    let product_descriptors = ProductArtifactRole::ALL
        .into_iter()
        .map(|role| {
            let bytes = &product_artifacts[&role];
            json!({
                "role": role.as_str(),
                "fileName": format!("{}.bin", role.as_str()),
                "byteLength": bytes.len(),
                "sha256": hex::encode(Sha256::digest(bytes)),
            })
        })
        .collect::<Vec<_>>();
    let product_manifest = canonicalize(&json!({
        "schema": CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V2,
        "channel": "stable",
        "releaseSequence": 1,
        "releaseVersion": "2.0.0",
        "sourceRevision": "44".repeat(20),
        "targetOs": "macos",
        "targetArch": "arm64",
        "minimumMacOs": "14.0",
        "previousManifestSha256": null,
        "controllerProtocolVersion": 1,
        "guestManifestSha256": hex::encode(Sha256::digest(&guest_manifest)),
        "guestReleaseSequence": 1,
        "guestReleaseVersion": "2.0.0",
        "artifacts": product_descriptors,
    }));
    let product_signature = envelope_for(
        &product_key,
        product_key_id,
        &product_manifest,
        CURL_PRODUCT_RELEASE_SIGNING_CONTEXT_V2,
    );
    (
        CurlProductReleaseTrustRoot::new(product_key_id, &product_key.pk_hex())
            .expect("product trust root"),
        NativeShadowUpdateTrustRoot::new(guest_key_id, &guest_key.pk_hex())
            .expect("guest trust root"),
        product_manifest,
        product_signature,
        product_artifacts,
        guest_artifacts,
    )
}

fn serve_bootable_bundle(server: &LoopbackServer, fixture: &BootableFixture) {
    server.set_route(
        &format!("{BUNDLE_PATH}/{CURL_PRODUCT_INSTALLED_MANIFEST_FILE}"),
        200,
        fixture.2.clone(),
    );
    server.set_route(
        &format!("{BUNDLE_PATH}/{CURL_PRODUCT_INSTALLED_SIGNATURE_FILE}"),
        200,
        fixture.3.clone(),
    );
    for role in ProductArtifactRole::ALL {
        server.set_route(
            &format!("{BUNDLE_PATH}/{}.bin", role.as_str()),
            200,
            fixture.4[&role].clone(),
        );
    }
    for role in GuestArtifactRole::BOOTABLE_ALL {
        server.set_route(
            &format!("{BUNDLE_PATH}/guest/{}.bin", role.as_str()),
            200,
            fixture.5[&role].clone(),
        );
    }
}

#[test]
fn a_bootable_bundle_authenticates_both_domains_before_atomic_install() {
    let fixture = signed_bootable_fixture();
    let server = LoopbackServer::start();
    serve_bootable_bundle(&server, &fixture);
    let dir = FixtureDir::new("bootable-happy");
    let root = dir.path("root");
    let staging = dir.path("download-staging");

    let installed = download_and_install_bootable_curl_product_release(
        &server.base_url(),
        &root,
        &staging,
        &fixture.0,
        1,
        &fixture.1,
        1,
        REQUEST_TIMEOUT,
    )
    .expect("bootable bundle installs");

    assert_eq!(installed.product().release_sequence(), 1);
    assert_eq!(installed.guest_release_sequence(), 1);
    for role in GuestArtifactRole::BOOTABLE_ALL {
        assert_eq!(
            fs::read(installed.guest_artifact_path(role).expect("guest path"))
                .expect("installed guest bytes"),
            fixture.5[&role]
        );
    }
    assert!(!staging.exists());
    assert!(server
        .requests()
        .contains(&format!("{BUNDLE_PATH}/guest/guest-root-disk.bin")));
}

#[test]
fn a_forged_embedded_guest_signature_stops_before_guest_artifact_downloads() {
    let fixture = signed_bootable_fixture_with_guest_signer(false);
    let server = LoopbackServer::start();
    serve_bootable_bundle(&server, &fixture);
    let dir = FixtureDir::new("bootable-forged-guest");
    let root = dir.path("root");
    let staging = dir.path("download-staging");

    let error = download_and_install_bootable_curl_product_release(
        &server.base_url(),
        &root,
        &staging,
        &fixture.0,
        1,
        &fixture.1,
        1,
        REQUEST_TIMEOUT,
    )
    .expect_err("forged guest signature is rejected");

    assert!(matches!(
        error,
        CurlProductTransportError::Install(CurlProductInstallError::GuestVerify(_))
    ));
    assert!(server
        .requests()
        .iter()
        .all(|path| !path.starts_with(&format!("{BUNDLE_PATH}/guest/"))));
    assert_root_untouched(&root);
    assert!(!staging.exists());
}

#[test]
fn a_release_bundle_downloads_into_staging_and_installs_end_to_end() {
    let fixture = signed_product_fixture();
    let server = LoopbackServer::start();
    serve_bundle(&server, &fixture);
    let dir = FixtureDir::new("happy");
    let root = dir.path("root");
    let staging = dir.path("download-staging");

    let installed =
        download(&server.base_url(), &root, &staging, &fixture, 1).expect("bundle installs");

    assert_eq!(installed.release_sequence(), 1);
    assert_eq!(installed.release_version(), "1.0.0");
    let state = read_installed_curl_product_state(&root)
        .expect("state readable")
        .expect("state present");
    assert_eq!(state.release_sequence(), 1);
    assert_eq!(state.manifest_sha256(), manifest_sha256_hex(&fixture));

    let version_dir = root
        .join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY)
        .join(state.version_directory());
    for role in ProductArtifactRole::ALL {
        let installed_bytes = fs::read(version_dir.join(format!("{}.bin", role.as_str())))
            .expect("installed artifact");
        assert_eq!(installed_bytes, fixture.3[&role], "{}", role.as_str());
    }
    assert_eq!(
        fs::read(version_dir.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE))
            .expect("installed manifest"),
        fixture.1
    );
    assert_eq!(
        fs::read(version_dir.join(CURL_PRODUCT_INSTALLED_SIGNATURE_FILE))
            .expect("installed signature"),
        fixture.2
    );

    // Downloaded bytes lived only in the transient staging directory, and
    // it is gone after adoption.
    assert!(!staging.exists());
    assert_eq!(server.requests(), expected_bundle_requests());
}

#[test]
fn a_signed_successor_downloads_and_installs_over_the_active_release() {
    let fixture = signed_product_fixture();
    let server = LoopbackServer::start();
    serve_bundle(&server, &fixture);
    let dir = FixtureDir::new("successor");
    let root = dir.path("root");
    let staging = dir.path("download-staging");
    download(&server.base_url(), &root, &staging, &fixture, 1).expect("first install");

    let successor = signed_successor_fixture(&manifest_sha256_hex(&fixture));
    serve_bundle(&server, &successor);
    server.clear_requests();

    let installed =
        download(&server.base_url(), &root, &staging, &successor, 1).expect("successor installs");

    assert_eq!(installed.release_sequence(), 2);
    assert_eq!(installed.release_version(), "1.0.1");
    let state = read_installed_curl_product_state(&root)
        .expect("state readable")
        .expect("state present");
    assert_eq!(state.release_sequence(), 2);
    assert_eq!(state.manifest_sha256(), manifest_sha256_hex(&successor));
    assert!(!staging.exists());
    assert_eq!(server.requests(), expected_bundle_requests());
}

#[test]
fn a_corrupt_install_state_aborts_before_any_network_request() {
    let fixture = signed_product_fixture();
    let server = LoopbackServer::start();
    serve_bundle(&server, &fixture);
    let dir = FixtureDir::new("corrupt-state");
    let root = dir.path("root");
    let staging = dir.path("download-staging");
    fs::create_dir_all(&root).expect("create root");
    fs::write(root.join(CURL_PRODUCT_INSTALL_STATE_FILE), b"{not json").expect("corrupt state");

    let error = download(&server.base_url(), &root, &staging, &fixture, 1)
        .expect_err("corrupt state fails closed");

    assert!(
        matches!(
            &error,
            CurlProductTransportError::Install(CurlProductInstallError::State(_))
        ),
        "unexpected error: {error:?}"
    );
    // Fail-closed ordering: the durable state was inspected before any
    // network request, so the server saw nothing at all.
    assert_eq!(server.requests(), Vec::<String>::new());
    assert!(!staging.exists());
    assert_eq!(
        fs::read(root.join(CURL_PRODUCT_INSTALL_STATE_FILE)).expect("state preserved"),
        b"{not json"
    );
}

#[test]
fn a_forged_signature_aborts_before_any_artifact_download() {
    let (trust_root, manifest, _signature, artifacts) = signed_product_fixture();
    let attacker = SigningKeyV2::from_dev_id("attacker-not-the-trust-root");
    let forged_signature = product_signature_for(&attacker, KAT_KEY_ID, &manifest);
    let forged: ProductFixture = (trust_root, manifest, forged_signature, artifacts);
    let server = LoopbackServer::start();
    serve_bundle(&server, &forged);
    let dir = FixtureDir::new("forged");
    let root = dir.path("root");
    let staging = dir.path("download-staging");

    let error = download(&server.base_url(), &root, &staging, &forged, 1)
        .expect_err("forged signature is rejected");

    assert!(
        matches!(
            &error,
            CurlProductTransportError::Verify(CurlProductReleaseVerifyError::UntrustedKey)
        ),
        "unexpected error: {error:?}"
    );
    // The authentication gate ran before any artifact request: only the
    // manifest and detached signature were fetched.
    assert_eq!(server.requests(), expected_bundle_requests()[..2].to_vec());
    assert_root_untouched(&root);
    assert!(!staging.exists());
}

#[test]
fn a_replayed_bundle_is_rejected_before_artifact_download() {
    let fixture = signed_product_fixture();
    let server = LoopbackServer::start();
    serve_bundle(&server, &fixture);
    let dir = FixtureDir::new("replay");
    let root = dir.path("root");
    let staging = dir.path("download-staging");
    download(&server.base_url(), &root, &staging, &fixture, 1).expect("first install");
    server.clear_requests();

    let error = download(&server.base_url(), &root, &staging, &fixture, 1)
        .expect_err("replayed bundle is rejected");

    assert!(
        matches!(
            &error,
            CurlProductTransportError::Verify(CurlProductReleaseVerifyError::VersionChain(_))
        ),
        "unexpected error: {error:?}"
    );
    assert_eq!(server.requests(), expected_bundle_requests()[..2].to_vec());
    let state = read_installed_curl_product_state(&root)
        .expect("state readable")
        .expect("state present");
    assert_eq!(state.release_sequence(), 1);
    assert!(!staging.exists());
}

#[test]
fn a_tampered_artifact_with_a_success_status_is_rejected() {
    let fixture = signed_product_fixture();
    let server = LoopbackServer::start();
    serve_bundle(&server, &fixture);
    // Same declared length, different bytes, HTTP 200: transport-level
    // success signals must not survive the digest check.
    let mut tampered = fixture.3[&ProductArtifactRole::HostCli].clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    server.set_route(&artifact_route(ProductArtifactRole::HostCli), 200, tampered);
    let dir = FixtureDir::new("tampered");
    let root = dir.path("root");
    let staging = dir.path("download-staging");

    let error = download(&server.base_url(), &root, &staging, &fixture, 1)
        .expect_err("tampered artifact is rejected");

    match &error {
        CurlProductTransportError::Install(CurlProductInstallError::Verify(
            CurlProductReleaseVerifyError::ArtifactMismatch(message),
        )) => {
            assert!(message.contains("SHA-256 mismatch"), "message: {message}");
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert_root_untouched(&root);
    assert!(!staging.exists());
}

#[test]
fn an_artifact_stream_longer_than_its_declared_length_is_aborted() {
    let fixture = signed_product_fixture();
    let server = LoopbackServer::start();
    serve_bundle(&server, &fixture);
    let mut oversized = fixture.3[&ProductArtifactRole::HostNode].clone();
    oversized.extend_from_slice(b"trailing-bytes-past-the-signed-length");
    server.set_route(
        &artifact_route(ProductArtifactRole::HostNode),
        200,
        oversized,
    );
    let dir = FixtureDir::new("overlong");
    let root = dir.path("root");
    let staging = dir.path("download-staging");

    let error = download(&server.base_url(), &root, &staging, &fixture, 1)
        .expect_err("over-long artifact stream is aborted");

    match &error {
        CurlProductTransportError::Download(message) => {
            assert!(message.contains("host-node.bin"), "message: {message}");
            assert!(
                message.contains("declared byte length"),
                "message: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
    // The abort happened at host-node; later artifacts were never fetched.
    assert_eq!(server.requests(), expected_bundle_requests()[..4].to_vec());
    assert_root_untouched(&root);
    assert!(!staging.exists());
}

#[test]
fn an_artifact_stream_shorter_than_its_declared_length_is_rejected() {
    let fixture = signed_product_fixture();
    let server = LoopbackServer::start();
    serve_bundle(&server, &fixture);
    let mut truncated = fixture.3[&ProductArtifactRole::HostWalletAgent].clone();
    truncated.pop();
    server.set_route(
        &artifact_route(ProductArtifactRole::HostWalletAgent),
        200,
        truncated,
    );
    let dir = FixtureDir::new("truncated");
    let root = dir.path("root");
    let staging = dir.path("download-staging");

    let error = download(&server.base_url(), &root, &staging, &fixture, 1)
        .expect_err("truncated artifact stream is rejected");

    match &error {
        CurlProductTransportError::Download(message) => {
            assert!(
                message.contains("host-wallet-agent.bin"),
                "message: {message}"
            );
            assert!(
                message.contains("declared byte length"),
                "message: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert_root_untouched(&root);
    assert!(!staging.exists());
}

#[test]
fn a_missing_artifact_download_is_a_transport_failure() {
    let fixture = signed_product_fixture();
    let server = LoopbackServer::start();
    serve_bundle(&server, &fixture);
    server.remove_route(&artifact_route(ProductArtifactRole::HostController));
    let dir = FixtureDir::new("missing-artifact");
    let root = dir.path("root");
    let staging = dir.path("download-staging");

    let error = download(&server.base_url(), &root, &staging, &fixture, 1)
        .expect_err("missing artifact fails the download");

    match &error {
        CurlProductTransportError::Download(message) => {
            assert!(
                message.contains("host-controller.bin"),
                "message: {message}"
            );
            assert!(message.contains("404"), "message: {message}");
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert_root_untouched(&root);
    assert!(!staging.exists());
}

#[test]
fn an_http_error_status_on_the_manifest_is_a_transport_failure() {
    let fixture = signed_product_fixture();
    let server = LoopbackServer::start();
    server.set_route(
        &format!("{BUNDLE_PATH}/{CURL_PRODUCT_INSTALLED_MANIFEST_FILE}"),
        500,
        b"transient upstream failure".to_vec(),
    );
    let dir = FixtureDir::new("manifest-500");
    let root = dir.path("root");
    let staging = dir.path("download-staging");

    let error = download(&server.base_url(), &root, &staging, &fixture, 1)
        .expect_err("manifest status error fails the download");

    match &error {
        CurlProductTransportError::Download(message) => {
            assert!(
                message.contains("release-manifest.json"),
                "message: {message}"
            );
            assert!(message.contains("500"), "message: {message}");
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert_eq!(server.requests(), expected_bundle_requests()[..1].to_vec());
    assert_root_untouched(&root);
    assert!(!staging.exists());
}

#[test]
fn a_manifest_download_larger_than_the_contract_cap_is_aborted() {
    let fixture = signed_product_fixture();
    let server = LoopbackServer::start();
    server.set_route(
        &format!("{BUNDLE_PATH}/{CURL_PRODUCT_INSTALLED_MANIFEST_FILE}"),
        200,
        vec![b'{'; MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES + 1],
    );
    let dir = FixtureDir::new("manifest-cap");
    let root = dir.path("root");
    let staging = dir.path("download-staging");

    let error = download(&server.base_url(), &root, &staging, &fixture, 1)
        .expect_err("oversized manifest download is aborted");

    match &error {
        CurlProductTransportError::Download(message) => {
            assert!(
                message.contains("release-manifest.json"),
                "message: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert_eq!(server.requests(), expected_bundle_requests()[..1].to_vec());
    assert_root_untouched(&root);
    assert!(!staging.exists());
}

#[test]
fn an_unreachable_server_is_a_transport_failure() {
    let fixture = signed_product_fixture();
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve a loopback port");
    let port = listener.local_addr().expect("loopback address").port();
    drop(listener);
    let dir = FixtureDir::new("unreachable");
    let root = dir.path("root");
    let staging = dir.path("download-staging");

    let error = download(
        &format!("http://127.0.0.1:{port}{BUNDLE_PATH}"),
        &root,
        &staging,
        &fixture,
        1,
    )
    .expect_err("unreachable server fails the download");

    assert!(
        matches!(&error, CurlProductTransportError::Download(_)),
        "unexpected error: {error:?}"
    );
    assert_root_untouched(&root);
    assert!(!staging.exists());
}

#[test]
fn leftover_download_staging_residue_cannot_poison_the_install() {
    let fixture = signed_product_fixture();
    let server = LoopbackServer::start();
    serve_bundle(&server, &fixture);
    let dir = FixtureDir::new("residue");
    let root = dir.path("root");
    let staging = dir.path("download-staging");
    fs::create_dir_all(staging.join("nested")).expect("residue dir");
    fs::write(staging.join("host-cli.bin"), b"poisoned residue").expect("residue artifact");

    let installed =
        download(&server.base_url(), &root, &staging, &fixture, 1).expect("bundle installs");

    let state = read_installed_curl_product_state(&root)
        .expect("state readable")
        .expect("state present");
    let installed_cli = fs::read(
        root.join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY)
            .join(state.version_directory())
            .join("host-cli.bin"),
    )
    .expect("installed host-cli");
    assert_eq!(installed_cli, fixture.3[&ProductArtifactRole::HostCli]);
    assert_eq!(installed.release_sequence(), 1);
    assert!(!staging.exists());
}

#[test]
fn a_non_http_url_is_rejected_without_any_request() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("bad-url");
    let root = dir.path("root");
    let staging = dir.path("download-staging");

    for base_url in ["ftp://127.0.0.1/bundle", "file:///tmp/bundle", "not a url"] {
        let error =
            download(base_url, &root, &staging, &fixture, 1).expect_err("non-http url is rejected");
        match &error {
            CurlProductTransportError::Url(message) => {
                assert!(message.contains("http"), "message: {message}");
            }
            other => panic!("unexpected error for {base_url}: {other:?}"),
        }
    }
    assert_root_untouched(&root);
    assert!(!staging.exists());
}

#[test]
fn a_download_staging_dir_inside_the_install_root_is_rejected() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("staging-in-root");
    let root = dir.path("root");
    let staging = root.join("download-staging");

    let error = download("http://127.0.0.1:9/bundle", &root, &staging, &fixture, 1)
        .expect_err("staging inside the install root is rejected");

    match &error {
        CurlProductTransportError::Io(message) => {
            assert!(message.contains("install root"), "message: {message}");
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert_root_untouched(&root);
    assert!(!staging.exists());
}
