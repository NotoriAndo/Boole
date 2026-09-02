//! CURL.2-TRANSPORT — bundle download/staging that drives the CURL.2-CORE
//! installer.
//!
//! This module fetches a curl-product release bundle over HTTP(S) and hands
//! it to the frozen v1 installer or the separate bootable-v2 installer.
//! Transport is never
//! trust: the URL, the HTTP status code and the server-chosen bytes carry no
//! authority. Every downloaded byte is verified by the CURL.1 release
//! verifier (injected Ed25519 trust root, canonical manifest, exact SHA-256
//! digests) and adopted only by the CURL.2-CORE installer's atomic,
//! fail-closed adoption path.
//!
//! Fail-closed download order:
//!
//! 1. validate the base URL shape (http/https only) and the staging layout;
//! 2. read the durable install state — a corrupt state aborts before any
//!    network request is made;
//! 3. download the manifest and detached signature under the frozen contract
//!    caps (1 MiB / 4 KiB) into memory only;
//! 4. authenticate them against the injected trust root and the replay
//!    floor — an unauthenticated bundle aborts before any artifact request;
//! 5. download exactly the artifacts the signed manifest declares, each
//!    bounded by its signed `byteLength`, into a transient download staging
//!    directory that is never the install tree;
//! 6. run the CURL.2-CORE installer, which re-verifies the full release and
//!    adopts it atomically; and
//! 7. remove the download staging directory whether the install succeeded
//!    or failed.
//!
//! No production trust root, release upload or public network interaction
//! belongs here: closed-local loopback tests with a KAT key are the only
//! exercised transport.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use boole_core::{
    authenticate_bootable_curl_product_release, authenticate_curl_product_release,
    authenticate_direct_boot_curl_product_release,
    authenticate_staged_bootable_native_shadow_update,
    authenticate_staged_direct_boot_native_shadow_update, install_bootable_curl_product_release,
    install_curl_product_release, install_direct_boot_curl_product_release,
    open_verified_installed_bootable_curl_product_release,
    open_verified_installed_curl_product_release,
    open_verified_installed_direct_boot_curl_product_release, read_installed_curl_product_state,
    AuthenticatedCurlProductRelease, CurlProductInstallError, CurlProductReleaseFloor,
    CurlProductReleaseTrustRoot, CurlProductReleaseVerifyError, GuestArtifactRole,
    InstalledBootableCurlProduct, InstalledCurlProduct, NativeShadowUpdateFloor,
    NativeShadowUpdateTrustRoot, ProductArtifactRole, CURL_PRODUCT_INSTALLED_MANIFEST_FILE,
    CURL_PRODUCT_INSTALLED_SIGNATURE_FILE, CURL_PRODUCT_INSTALL_STATE_FILE,
    MAX_CURL_PRODUCT_RELEASE_DETACHED_SIGNATURE_BYTES, MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES,
};

#[derive(Clone, Copy)]
enum TransportGuestContract {
    BootableV2,
    DirectBootV3,
}

impl TransportGuestContract {
    const fn roles(self) -> &'static [GuestArtifactRole] {
        match self {
            Self::BootableV2 => &GuestArtifactRole::BOOTABLE_ALL,
            Self::DirectBootV3 => &GuestArtifactRole::DIRECT_BOOT_ALL,
        }
    }
}

/// Transport-layer rejection. `Verify`/`Install` wrap the CURL.1 and
/// CURL.2-CORE rejections unchanged; the other variants are pure transport
/// diagnostics and never grant trust.
#[derive(Debug, thiserror::Error)]
pub enum CurlProductTransportError {
    #[error("download url rejected: {0}")]
    Url(String),
    #[error("bundle download failed: {0}")]
    Download(String),
    #[error("release rejected: {0}")]
    Verify(#[from] CurlProductReleaseVerifyError),
    #[error("install rejected: {0}")]
    Install(#[from] CurlProductInstallError),
    #[error("download staging operation failed: {0}")]
    Io(String),
}

/// Download the release bundle published under `base_url` into
/// `download_staging_dir` and drive the verified atomic installer against
/// `install_root`. Returns the adopted release on success.
pub fn download_and_install_curl_product_release(
    base_url: &str,
    install_root: &Path,
    download_staging_dir: &Path,
    trust_root: &CurlProductReleaseTrustRoot,
    first_install_minimum_sequence: u64,
    request_timeout: Duration,
) -> Result<InstalledCurlProduct, CurlProductTransportError> {
    let base_url = validated_base_url(base_url)?;
    check_staging_layout(install_root, download_staging_dir)?;
    let floor = release_floor(install_root, first_install_minimum_sequence)?;

    let client = reqwest::blocking::Client::builder()
        .timeout(request_timeout)
        .build()
        .map_err(|error| {
            CurlProductTransportError::Download(format!("http client build failed: {error}"))
        })?;
    let manifest_raw = fetch_capped(
        &client,
        &base_url,
        CURL_PRODUCT_INSTALLED_MANIFEST_FILE,
        MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES,
    )?;
    let signature_raw = fetch_capped(
        &client,
        &base_url,
        CURL_PRODUCT_INSTALLED_SIGNATURE_FILE,
        MAX_CURL_PRODUCT_RELEASE_DETACHED_SIGNATURE_BYTES,
    )?;
    let authenticated =
        authenticate_curl_product_release(&manifest_raw, &signature_raw, trust_root, &floor)?;

    // Only an authenticated bundle reaches the staging directory. From here
    // on the staging directory exists, so it is removed on every outcome.
    let outcome = download_artifacts_and_install(
        &client,
        &base_url,
        &authenticated,
        install_root,
        download_staging_dir,
        &manifest_raw,
        &signature_raw,
        trust_root,
        first_install_minimum_sequence,
    );
    let _ = fs::remove_dir_all(download_staging_dir);
    outcome
}

/// Download and atomically install the product-v2 envelope plus its exact
/// signed bootable guest-v2 payload. Product artifacts live at `base_url`;
/// guest artifacts live below the fixed `base_url/guest` transport prefix.
/// Both signature domains are authenticated before their artifact requests.
#[allow(clippy::too_many_arguments)]
pub fn download_and_install_bootable_curl_product_release(
    base_url: &str,
    install_root: &Path,
    download_staging_dir: &Path,
    product_trust_root: &CurlProductReleaseTrustRoot,
    first_product_sequence: u64,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
    first_guest_sequence: u64,
    request_timeout: Duration,
) -> Result<InstalledBootableCurlProduct, CurlProductTransportError> {
    download_and_install_bootable_curl_product_release_for_contract(
        base_url,
        install_root,
        download_staging_dir,
        product_trust_root,
        first_product_sequence,
        guest_trust_root,
        first_guest_sequence,
        request_timeout,
        TransportGuestContract::BootableV2,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn download_and_install_direct_boot_curl_product_release(
    base_url: &str,
    install_root: &Path,
    download_staging_dir: &Path,
    product_trust_root: &CurlProductReleaseTrustRoot,
    first_product_sequence: u64,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
    first_guest_sequence: u64,
    request_timeout: Duration,
) -> Result<InstalledBootableCurlProduct, CurlProductTransportError> {
    download_and_install_bootable_curl_product_release_for_contract(
        base_url,
        install_root,
        download_staging_dir,
        product_trust_root,
        first_product_sequence,
        guest_trust_root,
        first_guest_sequence,
        request_timeout,
        TransportGuestContract::DirectBootV3,
    )
}

#[allow(clippy::too_many_arguments)]
fn download_and_install_bootable_curl_product_release_for_contract(
    base_url: &str,
    install_root: &Path,
    download_staging_dir: &Path,
    product_trust_root: &CurlProductReleaseTrustRoot,
    first_product_sequence: u64,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
    first_guest_sequence: u64,
    request_timeout: Duration,
    contract: TransportGuestContract,
) -> Result<InstalledBootableCurlProduct, CurlProductTransportError> {
    let base_url = validated_base_url(base_url)?;
    check_staging_layout(install_root, download_staging_dir)?;
    let product_floor = release_floor(install_root, first_product_sequence)?;
    let guest_floor = bootable_guest_floor(
        install_root,
        product_trust_root,
        guest_trust_root,
        first_guest_sequence,
        contract,
    )?;
    let client = reqwest::blocking::Client::builder()
        .timeout(request_timeout)
        .build()
        .map_err(|error| {
            CurlProductTransportError::Download(format!("http client build failed: {error}"))
        })?;
    let manifest_raw = fetch_capped(
        &client,
        &base_url,
        CURL_PRODUCT_INSTALLED_MANIFEST_FILE,
        MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES,
    )?;
    let signature_raw = fetch_capped(
        &client,
        &base_url,
        CURL_PRODUCT_INSTALLED_SIGNATURE_FILE,
        MAX_CURL_PRODUCT_RELEASE_DETACHED_SIGNATURE_BYTES,
    )?;
    let authenticated = match contract {
        TransportGuestContract::BootableV2 => authenticate_bootable_curl_product_release(
            &manifest_raw,
            &signature_raw,
            product_trust_root,
            &product_floor,
        )?,
        TransportGuestContract::DirectBootV3 => authenticate_direct_boot_curl_product_release(
            &manifest_raw,
            &signature_raw,
            product_trust_root,
            &product_floor,
        )?,
    };
    let outcome = download_bootable_artifacts_and_install(
        &client,
        &base_url,
        authenticated,
        install_root,
        download_staging_dir,
        &manifest_raw,
        &signature_raw,
        product_trust_root,
        first_product_sequence,
        guest_trust_root,
        first_guest_sequence,
        &guest_floor,
        contract,
    );
    let _ = fs::remove_dir_all(download_staging_dir);
    outcome
}

fn bootable_guest_floor(
    install_root: &Path,
    product_trust_root: &CurlProductReleaseTrustRoot,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
    first_guest_sequence: u64,
    contract: TransportGuestContract,
) -> Result<NativeShadowUpdateFloor, CurlProductTransportError> {
    if read_installed_curl_product_state(install_root)?.is_none() {
        return NativeShadowUpdateFloor::first_install(first_guest_sequence)
            .map_err(|error| CurlProductTransportError::Install(error.into()));
    }
    let active = match contract {
        TransportGuestContract::BootableV2 => {
            open_verified_installed_bootable_curl_product_release(
                install_root,
                product_trust_root,
                guest_trust_root,
            )
        }
        TransportGuestContract::DirectBootV3 => {
            open_verified_installed_direct_boot_curl_product_release(
                install_root,
                product_trust_root,
                guest_trust_root,
            )
        }
    };
    match active {
        Ok(active) => NativeShadowUpdateFloor::installed(
            active.guest().release_sequence(),
            active.guest().manifest_sha256(),
        )
        .map_err(|error| CurlProductTransportError::Install(error.into())),
        Err(CurlProductInstallError::Verify(
            CurlProductReleaseVerifyError::InvalidSignatureContext,
        )) => {
            open_verified_installed_curl_product_release(install_root, product_trust_root)?;
            NativeShadowUpdateFloor::first_install(first_guest_sequence)
                .map_err(|error| CurlProductTransportError::Install(error.into()))
        }
        Err(error) => Err(error.into()),
    }
}

fn validated_base_url(base_url: &str) -> Result<String, CurlProductTransportError> {
    let parsed = reqwest::Url::parse(base_url).map_err(|error| {
        CurlProductTransportError::Url(format!("base url must be an absolute http(s) url: {error}"))
    })?;
    match parsed.scheme() {
        "http" | "https" => Ok(base_url.trim_end_matches('/').to_string()),
        other => Err(CurlProductTransportError::Url(format!(
            "scheme {other} is not http or https"
        ))),
    }
}

fn check_staging_layout(
    install_root: &Path,
    download_staging_dir: &Path,
) -> Result<(), CurlProductTransportError> {
    if download_staging_dir.starts_with(install_root) {
        return Err(CurlProductTransportError::Io(
            "the download staging directory must not live inside the install root".to_string(),
        ));
    }
    if install_root.starts_with(download_staging_dir) {
        return Err(CurlProductTransportError::Io(
            "the install root must not live inside the download staging directory".to_string(),
        ));
    }
    Ok(())
}

/// Build the replay floor from the durable install state before any network
/// request. The mapping mirrors the CURL.2-CORE installer exactly, so the
/// transport pre-check and the installer's own re-check agree on every input.
fn release_floor(
    install_root: &Path,
    first_install_minimum_sequence: u64,
) -> Result<CurlProductReleaseFloor, CurlProductTransportError> {
    match read_installed_curl_product_state(install_root)? {
        Some(state) => {
            CurlProductReleaseFloor::installed(state.release_sequence(), state.manifest_sha256())
                .map_err(|error| {
                    CurlProductTransportError::Install(CurlProductInstallError::State(format!(
                        "{CURL_PRODUCT_INSTALL_STATE_FILE} is internally inconsistent: {error}"
                    )))
                })
        }
        None => Ok(CurlProductReleaseFloor::first_install(
            first_install_minimum_sequence,
        )?),
    }
}

fn fetch(
    client: &reqwest::blocking::Client,
    base_url: &str,
    file_name: &str,
) -> Result<reqwest::blocking::Response, CurlProductTransportError> {
    let url = format!("{base_url}/{file_name}");
    let response = client.get(&url).send().map_err(|error| {
        CurlProductTransportError::Download(format!("{file_name} request failed: {error}"))
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(CurlProductTransportError::Download(format!(
            "{file_name} request returned HTTP status {}",
            status.as_u16()
        )));
    }
    Ok(response)
}

/// Fetch a metadata file into memory, aborting the stream as soon as it
/// exceeds `cap`. The caps come from the frozen CURL.1 contract, never from
/// server-controlled headers.
fn fetch_capped(
    client: &reqwest::blocking::Client,
    base_url: &str,
    file_name: &str,
    cap: usize,
) -> Result<Vec<u8>, CurlProductTransportError> {
    let mut response = fetch(client, base_url, file_name)?;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = response.read(&mut chunk).map_err(|error| {
            CurlProductTransportError::Download(format!(
                "{file_name} download failed mid-stream: {error}"
            ))
        })?;
        if read == 0 {
            return Ok(bytes);
        }
        if bytes.len() + read > cap {
            return Err(CurlProductTransportError::Download(format!(
                "{file_name} exceeds the {cap}-byte transport cap"
            )));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

/// Stream one artifact into the staging directory, bounded by the byte
/// length the signed manifest declares for it. Server-declared lengths and
/// status codes never widen this bound.
fn fetch_artifact(
    client: &reqwest::blocking::Client,
    base_url: &str,
    file_name: &str,
    declared_byte_length: u64,
    staging_path: &Path,
) -> Result<(), CurlProductTransportError> {
    let mut response = fetch(client, base_url, file_name)?;
    let mut file = fs::File::create(staging_path).map_err(|error| {
        CurlProductTransportError::Io(format!("cannot create {}: {error}", staging_path.display()))
    })?;
    let mut written: u64 = 0;
    let mut chunk = [0_u8; 65536];
    loop {
        let read = response.read(&mut chunk).map_err(|error| {
            CurlProductTransportError::Download(format!(
                "{file_name} download failed mid-stream: {error}"
            ))
        })?;
        if read == 0 {
            break;
        }
        written += read as u64;
        if written > declared_byte_length {
            return Err(CurlProductTransportError::Download(format!(
                "{file_name} stream exceeds its declared byte length {declared_byte_length}"
            )));
        }
        file.write_all(&chunk[..read]).map_err(|error| {
            CurlProductTransportError::Io(format!(
                "cannot write {}: {error}",
                staging_path.display()
            ))
        })?;
    }
    if written != declared_byte_length {
        return Err(CurlProductTransportError::Download(format!(
            "{file_name} stream ended at {written} bytes instead of its declared byte length \
             {declared_byte_length}"
        )));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn download_artifacts_and_install(
    client: &reqwest::blocking::Client,
    base_url: &str,
    authenticated: &AuthenticatedCurlProductRelease,
    install_root: &Path,
    download_staging_dir: &Path,
    manifest_raw: &[u8],
    signature_raw: &[u8],
    trust_root: &CurlProductReleaseTrustRoot,
    first_install_minimum_sequence: u64,
) -> Result<InstalledCurlProduct, CurlProductTransportError> {
    // Leftover residue from an interrupted run must never feed the install:
    // start from an empty staging directory unconditionally.
    if download_staging_dir.exists() {
        fs::remove_dir_all(download_staging_dir).map_err(|error| {
            CurlProductTransportError::Io(format!(
                "cannot clear leftover download staging {}: {error}",
                download_staging_dir.display()
            ))
        })?;
    }
    fs::create_dir_all(download_staging_dir).map_err(|error| {
        CurlProductTransportError::Io(format!(
            "cannot create download staging {}: {error}",
            download_staging_dir.display()
        ))
    })?;
    for role in ProductArtifactRole::ALL {
        let file_name = authenticated.artifact_file_name(role).ok_or_else(|| {
            CurlProductTransportError::Download(format!(
                "the signed manifest declares no file name for role {}",
                role.as_str()
            ))
        })?;
        let declared_byte_length = authenticated.artifact_byte_length(role).ok_or_else(|| {
            CurlProductTransportError::Download(format!(
                "the signed manifest declares no byte length for role {}",
                role.as_str()
            ))
        })?;
        fetch_artifact(
            client,
            base_url,
            file_name,
            declared_byte_length,
            &download_staging_dir.join(file_name),
        )?;
    }
    Ok(install_curl_product_release(
        install_root,
        manifest_raw,
        signature_raw,
        trust_root,
        first_install_minimum_sequence,
        download_staging_dir,
    )?)
}

#[allow(clippy::too_many_arguments)]
fn download_bootable_artifacts_and_install(
    client: &reqwest::blocking::Client,
    base_url: &str,
    mut authenticated_product: AuthenticatedCurlProductRelease,
    install_root: &Path,
    download_staging_dir: &Path,
    manifest_raw: &[u8],
    signature_raw: &[u8],
    product_trust_root: &CurlProductReleaseTrustRoot,
    first_product_sequence: u64,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
    first_guest_sequence: u64,
    guest_floor: &NativeShadowUpdateFloor,
    contract: TransportGuestContract,
) -> Result<InstalledBootableCurlProduct, CurlProductTransportError> {
    if download_staging_dir.exists() {
        fs::remove_dir_all(download_staging_dir).map_err(|error| {
            CurlProductTransportError::Io(format!(
                "cannot clear leftover download staging {}: {error}",
                download_staging_dir.display()
            ))
        })?;
    }
    let product_staging = download_staging_dir.join("product");
    let guest_staging = download_staging_dir.join("guest");
    fs::create_dir_all(&product_staging).map_err(|error| {
        CurlProductTransportError::Io(format!(
            "cannot create product download staging {}: {error}",
            product_staging.display()
        ))
    })?;
    for role in ProductArtifactRole::ALL {
        let file_name = authenticated_product
            .artifact_file_name(role)
            .ok_or_else(|| {
                CurlProductTransportError::Download(format!(
                    "the signed product declares no file name for role {}",
                    role.as_str()
                ))
            })?
            .to_string();
        let declared_byte_length = authenticated_product
            .artifact_byte_length(role)
            .ok_or_else(|| {
                CurlProductTransportError::Download(format!(
                    "the signed product declares no byte length for role {}",
                    role.as_str()
                ))
            })?;
        let path = product_staging.join(&file_name);
        fetch_artifact(client, base_url, &file_name, declared_byte_length, &path)?;
        authenticated_product.verify_artifact(
            role,
            fs::File::open(&path).map_err(|error| {
                CurlProductTransportError::Io(format!(
                    "cannot reopen staged product {}: {error}",
                    path.display()
                ))
            })?,
        )?;
    }
    let verified_product = authenticated_product.finish()?;
    let guest_manifest_path = product_staging.join(
        verified_product
            .artifact_file_name(ProductArtifactRole::GuestUpdateManifest)
            .expect("verified product contains guest manifest"),
    );
    let guest_signature_path = product_staging.join(
        verified_product
            .artifact_file_name(ProductArtifactRole::GuestUpdateSignature)
            .expect("verified product contains guest signature"),
    );
    let guest_manifest = fs::read(&guest_manifest_path).map_err(|error| {
        CurlProductTransportError::Io(format!(
            "cannot read staged guest manifest {}: {error}",
            guest_manifest_path.display()
        ))
    })?;
    let guest_signature = fs::read(&guest_signature_path).map_err(|error| {
        CurlProductTransportError::Io(format!(
            "cannot read staged guest signature {}: {error}",
            guest_signature_path.display()
        ))
    })?;
    let mut authenticated_guest = match contract {
        TransportGuestContract::BootableV2 => authenticate_staged_bootable_native_shadow_update(
            &guest_manifest,
            &guest_signature,
            guest_trust_root,
            guest_floor,
        ),
        TransportGuestContract::DirectBootV3 => {
            authenticate_staged_direct_boot_native_shadow_update(
                &guest_manifest,
                &guest_signature,
                guest_trust_root,
                guest_floor,
            )
        }
    }
    .map_err(|error| {
        CurlProductTransportError::Install(CurlProductInstallError::GuestVerify(error))
    })?;
    fs::create_dir(&guest_staging).map_err(|error| {
        CurlProductTransportError::Io(format!(
            "cannot create guest download staging {}: {error}",
            guest_staging.display()
        ))
    })?;
    let guest_base_url = format!("{base_url}/guest");
    for &role in contract.roles() {
        let file_name = authenticated_guest
            .artifact_file_name(role)
            .ok_or_else(|| {
                CurlProductTransportError::Download(format!(
                    "the signed guest declares no file name for role {}",
                    role.as_str()
                ))
            })?
            .to_string();
        let declared_byte_length =
            authenticated_guest
                .artifact_byte_length(role)
                .ok_or_else(|| {
                    CurlProductTransportError::Download(format!(
                        "the signed guest declares no byte length for role {}",
                        role.as_str()
                    ))
                })?;
        let path = guest_staging.join(&file_name);
        fetch_artifact(
            client,
            &guest_base_url,
            &file_name,
            declared_byte_length,
            &path,
        )?;
        authenticated_guest
            .verify_artifact(
                role,
                fs::File::open(&path).map_err(|error| {
                    CurlProductTransportError::Io(format!(
                        "cannot reopen staged guest {}: {error}",
                        path.display()
                    ))
                })?,
            )
            .map_err(|error| {
                CurlProductTransportError::Install(CurlProductInstallError::GuestVerify(error))
            })?;
    }
    authenticated_guest.finish().map_err(|error| {
        CurlProductTransportError::Install(CurlProductInstallError::GuestVerify(error))
    })?;

    match contract {
        TransportGuestContract::BootableV2 => install_bootable_curl_product_release(
            install_root,
            manifest_raw,
            signature_raw,
            product_trust_root,
            first_product_sequence,
            &product_staging,
            guest_trust_root,
            first_guest_sequence,
            &guest_staging,
        ),
        TransportGuestContract::DirectBootV3 => install_direct_boot_curl_product_release(
            install_root,
            manifest_raw,
            signature_raw,
            product_trust_root,
            first_product_sequence,
            &product_staging,
            guest_trust_root,
            first_guest_sequence,
            &guest_staging,
        ),
    }
    .map_err(Into::into)
}
