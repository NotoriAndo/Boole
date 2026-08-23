//! BF.6a — bounded, fail-closed package fetching into the node-owned CAS.
//!
//! This is availability plumbing only. It never mutates block, reward,
//! admission, replay, or fork-choice state. A response is staged only after
//! the wire root, canonical encoding, and content-derived root all agree.

use std::collections::{BTreeSet, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use boole_core::{
    CanonicalPackage, CompletePackageFetchIntentOutcome, Hex32, LocalPackageStore,
    LocalPackageStoreError, PackageRoot, StagePackageOutcome, DEFAULT_MAX_PENDING_PACKAGES,
    MAX_PACKAGE_REFERENCE_BYTES,
};
use boole_p2p::{Frame, FrameError, Transport};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::local_node::{head_summary, LocalNodeState};
use crate::p2p_egress::open_validated_conn;
use crate::p2p_ingress::{P2pIdentity, P2pMetrics};

const FETCH_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const FETCH_STOP_POLL_INTERVAL: Duration = Duration::from_millis(25);
const RECEIPT_FETCH_REFERENCE_PREFIX: &str = "receipt:";

/// BF.6a-only fixture block for proving package-availability plumbing before
/// BF.7 introduces any consensus block field. A receipt-free variant models
/// the existing Hash-only path; a receipt-bearing variant carries only the two
/// commitments needed to authorize package fetching.
///
/// This type is deliberately not serializable and is not a consensus block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageAvailabilityScaffoldBlock {
    ReceiptFree,
    ReceiptBearing {
        receipt_digest: Hex32,
        package_root: PackageRoot,
    },
}

impl PackageAvailabilityScaffoldBlock {
    pub fn receipt_free() -> Self {
        Self::ReceiptFree
    }

    pub fn receipt_bearing(receipt_digest: Hex32, package_root: PackageRoot) -> Self {
        Self::ReceiptBearing {
            receipt_digest,
            package_root,
        }
    }

    fn fetch_request(self) -> Result<Option<PackageFetchRequest>, PackageFetchingConfigError> {
        match self {
            Self::ReceiptFree => Ok(None),
            Self::ReceiptBearing {
                receipt_digest,
                package_root,
            } => PackageFetchRequest::new(
                package_root,
                format!(
                    "{RECEIPT_FETCH_REFERENCE_PREFIX}{}",
                    receipt_digest.to_hex()
                ),
            )
            .map(Some),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFetchRequest {
    root: PackageRoot,
    reference: String,
}

impl PackageFetchRequest {
    pub fn new(
        root: PackageRoot,
        reference: impl Into<String>,
    ) -> Result<Self, PackageFetchingConfigError> {
        let reference = reference.into();
        if reference.is_empty() {
            return Err(PackageFetchingConfigError::EmptyReference);
        }
        if reference.len() > MAX_PACKAGE_REFERENCE_BYTES {
            return Err(PackageFetchingConfigError::ReferenceTooLarge {
                size: reference.len(),
                max: MAX_PACKAGE_REFERENCE_BYTES,
            });
        }
        Ok(Self { root, reference })
    }

    pub fn root(&self) -> PackageRoot {
        self.root
    }

    pub fn reference(&self) -> &str {
        &self.reference
    }
}

pub struct PackageFetchingConfig {
    pub(crate) store: LocalPackageStore,
    pub(crate) requests: Vec<PackageFetchRequest>,
    pub(crate) retry_interval: Duration,
}

impl PackageFetchingConfig {
    /// Derive node-owned fetch authority directly from receipt-bearing BF.6a
    /// scaffold blocks. Exact repeated observations are idempotent; one
    /// receipt digest naming conflicting package roots is rejected by the
    /// durable store before any network thread can start.
    ///
    /// BF.7 consensus block parsing is intentionally outside this API.
    pub fn from_scaffold_blocks(
        store: LocalPackageStore,
        blocks: impl IntoIterator<Item = PackageAvailabilityScaffoldBlock>,
    ) -> Result<Self, PackageFetchingConfigError> {
        let mut identities = BTreeSet::new();
        let mut requests = Vec::new();
        for block in blocks {
            let Some(request) = block.fetch_request()? else {
                continue;
            };
            if identities.insert((request.root, request.reference.clone())) {
                requests.push(request);
            }
        }
        Self::new(store, requests)
    }

    pub fn new(
        mut store: LocalPackageStore,
        requests: impl IntoIterator<Item = PackageFetchRequest>,
    ) -> Result<Self, PackageFetchingConfigError> {
        if !store.is_enabled() {
            return Err(PackageFetchingConfigError::StoreDisabled);
        }
        let requests: Vec<_> = requests.into_iter().collect();
        if requests.len() > DEFAULT_MAX_PENDING_PACKAGES {
            return Err(PackageFetchingConfigError::TooManyRequests {
                count: requests.len(),
                max: DEFAULT_MAX_PENDING_PACKAGES,
            });
        }
        let mut identities = BTreeSet::new();
        for request in &requests {
            if !identities.insert((request.root, request.reference.clone())) {
                return Err(PackageFetchingConfigError::DuplicateRequest {
                    root: request.root.to_hex(),
                    reference: request.reference.clone(),
                });
            }
        }
        let intake = requests
            .iter()
            .map(|request| (request.root, request.reference.clone()))
            .collect::<Vec<_>>();
        store
            .register_fetch_intents(&intake)
            .map_err(|error| PackageFetchingConfigError::IntentJournal(error.to_string()))?;
        let requests = store
            .fetch_intents()
            .iter()
            .map(|intent| PackageFetchRequest {
                root: intent.root(),
                reference: intent.reference().to_owned(),
            })
            .collect();
        Ok(Self {
            store,
            requests,
            retry_interval: FETCH_RETRY_INTERVAL,
        })
    }

    #[doc(hidden)]
    pub fn with_retry_interval(mut self, retry_interval: Duration) -> Self {
        self.retry_interval = retry_interval.max(FETCH_STOP_POLL_INTERVAL);
        self
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PackageFetchingConfigError {
    #[error("package fetching requires an enabled node-owned local package store")]
    StoreDisabled,
    #[error("package fetch reference must not be empty")]
    EmptyReference,
    #[error("package fetch reference is {size} bytes; maximum is {max}")]
    ReferenceTooLarge { size: usize, max: usize },
    #[error("package fetch request count is {count}; maximum is {max}")]
    TooManyRequests { count: usize, max: usize },
    #[error("duplicate package fetch request for root {root} and reference {reference}")]
    DuplicateRequest { root: String, reference: String },
    #[error("package fetch-intent journal is unavailable: {0}")]
    IntentJournal(String),
}

#[derive(Debug, Error)]
enum PackageFetchError {
    #[error(transparent)]
    Transport(#[from] FrameError),
    #[error("peer returned package root {actual} for requested root {expected}")]
    ResponseRootMismatch { expected: String, actual: String },
    #[error("peer returned a non-canonical package: {0}")]
    InvalidCanonicalPackage(String),
    #[error("peer package content root {actual} does not match requested root {expected}")]
    ContentRootMismatch { expected: String, actual: String },
    #[error("peer returned an unexpected frame instead of Package")]
    UnexpectedFrame,
}

enum PeerFetchOutcome {
    Available(CanonicalPackage),
    Unavailable,
}

pub(crate) fn spawn_package_fetch_thread(
    config: PackageFetchingConfig,
    peers: Vec<SocketAddr>,
    identity: P2pIdentity,
    state: Arc<RwLock<LocalNodeState>>,
    stop: Arc<AtomicBool>,
    metrics: Arc<P2pMetrics>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("boole-p2p-package-fetch".to_string())
        .spawn(move || package_fetch_loop(config, peers, identity, state, stop, metrics))
        .expect("spawn boole-p2p package fetch thread")
}

fn package_fetch_loop(
    mut config: PackageFetchingConfig,
    peers: Vec<SocketAddr>,
    identity: P2pIdentity,
    state: Arc<RwLock<LocalNodeState>>,
    stop: Arc<AtomicBool>,
    metrics: Arc<P2pMetrics>,
) {
    let mut queue = VecDeque::from(std::mem::take(&mut config.requests));
    while !stop.load(Ordering::Relaxed) {
        let Some(request) = queue.pop_front() else {
            sleep_until_retry_or_stop(config.retry_interval, &stop);
            continue;
        };

        match already_staged(&config.store, &request) {
            Ok(true) => match complete_intent(&mut config.store, &request) {
                Ok(()) => {
                    metrics
                        .package_fetch_recovered
                        .fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                Err(_) => {
                    metrics
                        .package_fetch_store_errors
                        .fetch_add(1, Ordering::Relaxed);
                    queue.push_back(request);
                    sleep_until_retry_or_stop(config.retry_interval, &stop);
                    continue;
                }
            },
            Ok(false) => {}
            Err(_) => {
                metrics
                    .package_fetch_store_errors
                    .fetch_add(1, Ordering::Relaxed);
                queue.push_back(request);
                sleep_until_retry_or_stop(config.retry_interval, &stop);
                continue;
            }
        }

        let mut fetched = None;
        for peer in &peers {
            let head = head_summary(&state.blocking_read());
            match fetch_from_peer(peer, &identity, head, request.root) {
                Ok(PeerFetchOutcome::Available(package)) => {
                    fetched = Some(package);
                    break;
                }
                Ok(PeerFetchOutcome::Unavailable) => {
                    metrics
                        .package_fetch_unavailable
                        .fetch_add(1, Ordering::Relaxed);
                }
                Err(
                    PackageFetchError::ResponseRootMismatch { .. }
                    | PackageFetchError::InvalidCanonicalPackage(_)
                    | PackageFetchError::ContentRootMismatch { .. }
                    | PackageFetchError::UnexpectedFrame,
                ) => {
                    metrics
                        .package_fetch_invalid
                        .fetch_add(1, Ordering::Relaxed);
                }
                Err(PackageFetchError::Transport(_)) => {
                    metrics
                        .package_fetch_peer_failures
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        let completed = fetched.is_some_and(|package| {
            match config.store.stage(&package, request.reference()) {
                Ok(StagePackageOutcome::Staged | StagePackageOutcome::AlreadyPending) => {
                    match complete_intent(&mut config.store, &request) {
                        Ok(()) => {
                            metrics.package_fetch_staged.fetch_add(1, Ordering::Relaxed);
                            true
                        }
                        Err(_) => {
                            metrics
                                .package_fetch_store_errors
                                .fetch_add(1, Ordering::Relaxed);
                            false
                        }
                    }
                }
                Err(_) => {
                    metrics
                        .package_fetch_store_errors
                        .fetch_add(1, Ordering::Relaxed);
                    false
                }
            }
        });
        if !completed {
            queue.push_back(request);
            sleep_until_retry_or_stop(config.retry_interval, &stop);
        }
    }
}

fn complete_intent(
    store: &mut LocalPackageStore,
    request: &PackageFetchRequest,
) -> Result<(), LocalPackageStoreError> {
    match store.complete_fetch_intent(request.root, request.reference())? {
        CompletePackageFetchIntentOutcome::Completed => Ok(()),
        CompletePackageFetchIntentOutcome::NotPending => Err(LocalPackageStoreError::Corrupt(
            "fetch queue entry is absent from its durable intent authority".into(),
        )),
    }
}

fn already_staged(
    store: &LocalPackageStore,
    request: &PackageFetchRequest,
) -> Result<bool, LocalPackageStoreError> {
    let present = store
        .pending()
        .iter()
        .any(|entry| entry.root() == request.root && entry.reference() == request.reference);
    if !present {
        return Ok(false);
    }
    let bytes = store.read(request.root)?;
    let package = CanonicalPackage::from_canonical_bytes(&bytes).map_err(|error| {
        LocalPackageStoreError::Corrupt(format!(
            "pending CAS object is not a canonical package: {error}"
        ))
    })?;
    if package.root() != request.root {
        return Err(LocalPackageStoreError::ObjectRootMismatch {
            expected: request.root.to_hex(),
            actual: package.root().to_hex(),
        });
    }
    Ok(true)
}

fn fetch_from_peer(
    peer: &SocketAddr,
    identity: &P2pIdentity,
    head: boole_p2p::HeadSummary,
    requested_root: PackageRoot,
) -> Result<PeerFetchOutcome, PackageFetchError> {
    let (transport, mut conn, _peer_head) = open_validated_conn(peer, identity, head)?;
    let expected_root = requested_root.to_hex();
    transport.send_frame(
        &mut conn,
        &Frame::GetPackage {
            root: expected_root.clone(),
        },
    )?;
    let Frame::Package {
        root,
        canonical_bytes,
    } = transport.recv_frame(&mut conn)?
    else {
        return Err(PackageFetchError::UnexpectedFrame);
    };
    if root != expected_root {
        return Err(PackageFetchError::ResponseRootMismatch {
            expected: expected_root,
            actual: root,
        });
    }
    let Some(bytes) = canonical_bytes else {
        return Ok(PeerFetchOutcome::Unavailable);
    };
    let package = CanonicalPackage::from_canonical_bytes(&bytes)
        .map_err(|error| PackageFetchError::InvalidCanonicalPackage(error.to_string()))?;
    if package.root() != requested_root {
        return Err(PackageFetchError::ContentRootMismatch {
            expected: requested_root.to_hex(),
            actual: package.root().to_hex(),
        });
    }
    Ok(PeerFetchOutcome::Available(package))
}

fn sleep_until_retry_or_stop(duration: Duration, stop: &AtomicBool) {
    let mut remaining = duration;
    while remaining > Duration::ZERO && !stop.load(Ordering::Relaxed) {
        let step = remaining.min(FETCH_STOP_POLL_INTERVAL);
        thread::sleep(step);
        remaining = remaining.saturating_sub(step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boole_core::{LocalPackageStoreConfig, PackageFile};

    #[test]
    fn package_fetch_config_is_fail_closed() {
        let disabled = LocalPackageStore::open(
            "/path/not-touched-by-disabled-store",
            LocalPackageStoreConfig::default(),
        )
        .expect("disabled store");
        let package = CanonicalPackage::new(vec![PackageFile::new(b"a", b"b")]).expect("package");
        let request = PackageFetchRequest::new(package.root(), "receipt:one").expect("request");
        let error = match PackageFetchingConfig::new(disabled, [request]) {
            Ok(_) => panic!("disabled store must fail closed"),
            Err(error) => error,
        };
        assert_eq!(error, PackageFetchingConfigError::StoreDisabled);
        assert_eq!(
            PackageFetchRequest::new(package.root(), ""),
            Err(PackageFetchingConfigError::EmptyReference)
        );
    }
}
