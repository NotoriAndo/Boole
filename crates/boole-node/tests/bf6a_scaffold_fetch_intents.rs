//! BF.6a — receipt-bearing scaffold blocks automatically create durable
//! package fetch intents without touching the consensus block schema.

use std::fs;
use std::path::PathBuf;

use boole_core::{
    CanonicalPackage, Hex32, LocalPackageStore, LocalPackageStoreConfig, PackageFile,
    PACKAGE_FETCH_INTENTS_FILE,
};
use boole_node::{
    PackageAvailabilityScaffoldBlock, PackageFetchingConfig, PackageFetchingConfigError,
};
use boole_testkit::rand_suffix;

fn temp_parent(label: &str) -> PathBuf {
    let parent = std::env::temp_dir().join(format!(
        "boole-bf6a-scaffold-intents-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&parent).expect("create temporary parent");
    parent
}

fn store_config() -> LocalPackageStoreConfig {
    LocalPackageStoreConfig {
        enabled: true,
        max_pending_packages: 8,
        max_pending_bytes: 16 * 1024 * 1024,
    }
}

fn package(label: &str) -> CanonicalPackage {
    CanonicalPackage::new(vec![PackageFile::new(
        format!("src/{label}.rs"),
        format!("fn {label}() {{}}"),
    )])
    .expect("canonical package")
}

fn digest(byte: &str) -> Hex32 {
    Hex32::from_hex(&byte.repeat(32)).expect("hex32")
}

#[test]
fn receipt_bearing_scaffold_block_persists_fetch_intent_before_networking() {
    let parent = temp_parent("receipt");
    let root = parent.join("store");
    let package = package("receipt_bound");
    let receipt_digest = digest("11");
    let store = LocalPackageStore::open(&root, store_config()).expect("open package store");

    let config = PackageFetchingConfig::from_scaffold_blocks(
        store,
        [
            PackageAvailabilityScaffoldBlock::receipt_free(),
            PackageAvailabilityScaffoldBlock::receipt_bearing(receipt_digest, package.root()),
            // Bootstrap/reconnect may observe the same scaffold block again;
            // the durable authority must remain one exact intent.
            PackageAvailabilityScaffoldBlock::receipt_bearing(receipt_digest, package.root()),
        ],
    )
    .expect("derive and persist scaffold fetch intent");
    drop(config);

    let recovered = LocalPackageStore::open(&root, store_config()).expect("reopen package store");
    assert_eq!(recovered.fetch_intents().len(), 1);
    assert_eq!(recovered.fetch_intents()[0].root(), package.root());
    assert_eq!(
        recovered.fetch_intents()[0].reference(),
        format!("receipt:{}", receipt_digest.to_hex())
    );

    fs::remove_dir_all(parent).expect("remove temporary parent");
}

#[test]
fn receipt_free_scaffold_block_does_not_create_fetch_intent_authority() {
    let parent = temp_parent("receipt-free");
    let root = parent.join("store");
    let store = LocalPackageStore::open(&root, store_config()).expect("open package store");

    let config = PackageFetchingConfig::from_scaffold_blocks(
        store,
        [PackageAvailabilityScaffoldBlock::receipt_free()],
    )
    .expect("receipt-free scaffold is a no-op");
    drop(config);

    assert!(
        !root.join(PACKAGE_FETCH_INTENTS_FILE).exists(),
        "a receipt-free Hash scaffold block must not create fetch authority"
    );

    fs::remove_dir_all(parent).expect("remove temporary parent");
}

#[test]
fn one_receipt_digest_cannot_authorize_conflicting_package_roots() {
    let parent = temp_parent("conflict");
    let root = parent.join("store");
    let first = package("first");
    let second = package("second");
    let receipt_digest = digest("22");
    let store = LocalPackageStore::open(&root, store_config()).expect("open package store");

    let error = match PackageFetchingConfig::from_scaffold_blocks(
        store,
        [
            PackageAvailabilityScaffoldBlock::receipt_bearing(receipt_digest, first.root()),
            PackageAvailabilityScaffoldBlock::receipt_bearing(receipt_digest, second.root()),
        ],
    ) {
        Ok(_) => panic!("one receipt digest must not name two package roots"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        PackageFetchingConfigError::IntentJournal(_)
    ));
    assert!(
        !root.join(PACKAGE_FETCH_INTENTS_FILE).exists(),
        "conflicting scaffold input must fail before any authority is committed"
    );

    fs::remove_dir_all(parent).expect("remove temporary parent");
}
