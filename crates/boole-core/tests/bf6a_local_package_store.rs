use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::{
    h_protocol, AcknowledgePackageOutcome, CanonicalPackage, CompletePackageFetchIntentOutcome,
    LocalPackageStore, LocalPackageStoreConfig, LocalPackageStoreError, PackageFile,
    PendingCapacityPolicy, StagePackageOutcome, DEFAULT_MAX_PENDING_PACKAGES,
    MAX_FETCH_INTENT_SNAPSHOT_BYTES, MAX_PACKAGE_REFERENCE_BYTES, MAX_PENDING_SNAPSHOT_BYTES,
    PACKAGE_FETCH_INTENTS_FILE, PACKAGE_OBJECTS_DIRECTORY, PACKAGE_PENDING_FILE,
    PACKAGE_PENDING_TEMP_FILE, PACKAGE_SIDECAR_ROOT_DOMAIN, PENDING_CAPACITY_POLICY,
};

fn temporary_store_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "boole-bf6a-store-{label}-{}-{nonce}",
        std::process::id()
    ))
}

fn enabled_config(max_pending_packages: usize, max_pending_bytes: u64) -> LocalPackageStoreConfig {
    LocalPackageStoreConfig {
        enabled: true,
        max_pending_packages,
        max_pending_bytes,
    }
}

fn current_directory_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn local_package_store_is_default_off_and_does_not_touch_disk() {
    let root = temporary_store_path("default-off");
    assert!(!root.exists());

    let mut store = LocalPackageStore::open(&root, LocalPackageStoreConfig::default())
        .expect("disabled store opens without side effects");
    let package = CanonicalPackage::new(vec![PackageFile::new(b"answer.txt", b"42")])
        .expect("valid canonical package");

    assert!(!store.is_enabled());
    assert_eq!(
        store.stage(&package, "receipt:one"),
        Err(LocalPackageStoreError::Disabled)
    );
    assert_eq!(
        store.read(package.root()),
        Err(LocalPackageStoreError::Disabled)
    );
    assert_eq!(
        store.acknowledge(package.root(), "receipt:one"),
        Err(LocalPackageStoreError::Disabled)
    );
    assert!(
        !root.exists(),
        "OFF mode must be a byte-for-byte disk no-op"
    );
}

#[test]
fn staged_package_and_pending_reference_survive_reopen() {
    let root = temporary_store_path("reopen");
    let package = CanonicalPackage::new(vec![
        PackageFile::new(b"README.md", b"hello"),
        PackageFile::new(b"src/lib.rs", b"pub fn answer() -> u8 { 42 }"),
    ])
    .expect("valid canonical package");
    let expected_root = package.root();
    let expected_bytes = package.canonical_bytes().to_vec();

    {
        let mut store = LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024))
            .expect("enabled store opens");
        assert_eq!(
            store.stage(&package, "receipt:stable-task:1"),
            Ok(StagePackageOutcome::Staged)
        );
        assert_eq!(store.pending().len(), 1);
        assert_eq!(store.pending()[0].root(), expected_root);
        assert_eq!(store.pending()[0].size_bytes(), expected_bytes.len() as u64);
        assert_eq!(store.pending()[0].reference(), "receipt:stable-task:1");
        assert_eq!(
            store.read(expected_root).expect("stored package reads"),
            expected_bytes
        );
    }

    let reopened = LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024))
        .expect("durable package and pending queue recover");
    assert_eq!(reopened.pending().len(), 1);
    assert_eq!(reopened.pending()[0].root(), expected_root);
    assert_eq!(
        reopened.read(expected_root).expect("CAS bytes recover"),
        expected_bytes
    );

    std::fs::remove_dir_all(root).expect("remove test store");
}

#[test]
fn fetch_intent_clears_only_after_the_matching_root_and_reference_are_durably_staged() {
    let root = temporary_store_path("fetch-intent-cleanup-order");
    let package = CanonicalPackage::new(vec![PackageFile::new(b"answer.txt", b"cleanup ordering")])
        .expect("valid canonical package");
    let target_reference = "receipt:target";
    let mut store = LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024))
        .expect("enabled store opens");
    store
        .register_fetch_intents(&[(package.root(), target_reference.to_owned())])
        .expect("register durable fetch intent");

    store
        .stage(&package, "receipt:unrelated")
        .expect("same CAS root under an unrelated pending reference");
    assert!(
        store
            .complete_fetch_intent(package.root(), target_reference)
            .is_err(),
        "CAS bytes alone must not authorize intent cleanup before the exact pending pair exists"
    );
    assert_eq!(store.fetch_intents().len(), 1);

    store
        .stage(&package, target_reference)
        .expect("stage the exact root/reference pair");
    assert_eq!(
        store
            .complete_fetch_intent(package.root(), target_reference)
            .expect("complete after exact durable stage"),
        CompletePackageFetchIntentOutcome::Completed
    );
    assert!(store.fetch_intents().is_empty());

    let reopened = LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024))
        .expect("reopen cleaned intent snapshot");
    assert!(reopened.fetch_intents().is_empty());
    assert!(reopened
        .pending()
        .iter()
        .any(|entry| entry.root() == package.root() && entry.reference() == target_reference));
    std::fs::remove_dir_all(root).expect("remove test store");
}

#[test]
fn fetch_intent_authority_fails_closed_on_corrupt_torn_duplicate_conflict_and_caps() {
    let first =
        CanonicalPackage::new(vec![PackageFile::new(b"first", b"one")]).expect("first package");
    let second =
        CanonicalPackage::new(vec![PackageFile::new(b"second", b"two")]).expect("second package");

    let corrupt_root = temporary_store_path("fetch-intent-corrupt");
    LocalPackageStore::open(&corrupt_root, enabled_config(4, 1024 * 1024))
        .expect("initialize corrupt fixture");
    std::fs::write(corrupt_root.join(PACKAGE_FETCH_INTENTS_FILE), b"not-json")
        .expect("write corrupt snapshot");
    assert!(matches!(
        LocalPackageStore::open(&corrupt_root, enabled_config(4, 1024 * 1024)),
        Err(LocalPackageStoreError::Corrupt(_))
    ));

    let torn_root = temporary_store_path("fetch-intent-torn");
    LocalPackageStore::open(&torn_root, enabled_config(4, 1024 * 1024))
        .expect("initialize torn fixture");
    std::fs::write(
        torn_root.join(PACKAGE_FETCH_INTENTS_FILE),
        br#"{"schema":"boole.useful-work.package-fetch-intents.v1","entries":["#,
    )
    .expect("write torn snapshot");
    assert!(matches!(
        LocalPackageStore::open(&torn_root, enabled_config(4, 1024 * 1024)),
        Err(LocalPackageStoreError::Corrupt(_))
    ));

    let duplicate_root = temporary_store_path("fetch-intent-duplicate");
    LocalPackageStore::open(&duplicate_root, enabled_config(4, 1024 * 1024))
        .expect("initialize duplicate fixture");
    let duplicate = serde_json::json!({
        "schema": "boole.useful-work.package-fetch-intents.v1",
        "entries": [
            {"root": first.root().to_hex(), "reference": "receipt:same"},
            {"root": first.root().to_hex(), "reference": "receipt:same"}
        ]
    });
    std::fs::write(
        duplicate_root.join(PACKAGE_FETCH_INTENTS_FILE),
        serde_json::to_vec(&duplicate).expect("serialize duplicate snapshot"),
    )
    .expect("write duplicate snapshot");
    assert_eq!(
        LocalPackageStore::open(&duplicate_root, enabled_config(4, 1024 * 1024)).unwrap_err(),
        LocalPackageStoreError::Corrupt("duplicate root/reference fetch intent".into())
    );

    let conflicting_root = temporary_store_path("fetch-intent-conflict");
    LocalPackageStore::open(&conflicting_root, enabled_config(4, 1024 * 1024))
        .expect("initialize conflict fixture");
    let conflicting = serde_json::json!({
        "schema": "boole.useful-work.package-fetch-intents.v1",
        "entries": [
            {"root": first.root().to_hex(), "reference": "receipt:one-owner"},
            {"root": second.root().to_hex(), "reference": "receipt:one-owner"}
        ]
    });
    std::fs::write(
        conflicting_root.join(PACKAGE_FETCH_INTENTS_FILE),
        serde_json::to_vec(&conflicting).expect("serialize conflicting snapshot"),
    )
    .expect("write conflicting snapshot");
    assert_eq!(
        LocalPackageStore::open(&conflicting_root, enabled_config(4, 1024 * 1024)).unwrap_err(),
        LocalPackageStoreError::Corrupt(
            "one fetch-intent reference names conflicting roots".into()
        )
    );

    let count_root = temporary_store_path("fetch-intent-count-cap");
    LocalPackageStore::open(&count_root, enabled_config(4, 1024 * 1024))
        .expect("initialize count-cap fixture");
    let over_count = serde_json::json!({
        "schema": "boole.useful-work.package-fetch-intents.v1",
        "entries": [
            {"root": first.root().to_hex(), "reference": "receipt:first"},
            {"root": second.root().to_hex(), "reference": "receipt:second"}
        ]
    });
    std::fs::write(
        count_root.join(PACKAGE_FETCH_INTENTS_FILE),
        serde_json::to_vec(&over_count).expect("serialize over-count snapshot"),
    )
    .expect("write over-count snapshot");
    assert_eq!(
        LocalPackageStore::open(&count_root, enabled_config(1, 1024 * 1024)).unwrap_err(),
        LocalPackageStoreError::Corrupt("fetch-intent count exceeds configured bound".into())
    );

    let hard_count_root = temporary_store_path("fetch-intent-hard-count-cap");
    LocalPackageStore::open(&hard_count_root, enabled_config(128, 1024 * 1024))
        .expect("initialize hard-count-cap fixture");
    let over_hard_count = serde_json::json!({
        "schema": "boole.useful-work.package-fetch-intents.v1",
        "entries": (0..=DEFAULT_MAX_PENDING_PACKAGES)
            .map(|index| serde_json::json!({
                "root": first.root().to_hex(),
                "reference": format!("receipt:hard-cap:{index}")
            }))
            .collect::<Vec<_>>()
    });
    std::fs::write(
        hard_count_root.join(PACKAGE_FETCH_INTENTS_FILE),
        serde_json::to_vec(&over_hard_count).expect("serialize hard over-count snapshot"),
    )
    .expect("write hard over-count snapshot");
    assert_eq!(
        LocalPackageStore::open(&hard_count_root, enabled_config(128, 1024 * 1024)).unwrap_err(),
        LocalPackageStoreError::Corrupt("fetch-intent count exceeds configured bound".into())
    );

    let bytes_root = temporary_store_path("fetch-intent-bytes-cap");
    LocalPackageStore::open(&bytes_root, enabled_config(4, 1024 * 1024))
        .expect("initialize bytes-cap fixture");
    std::fs::write(
        bytes_root.join(PACKAGE_FETCH_INTENTS_FILE),
        vec![b'x'; MAX_FETCH_INTENT_SNAPSHOT_BYTES as usize + 1],
    )
    .expect("write over-sized snapshot");
    assert_eq!(
        LocalPackageStore::open(&bytes_root, enabled_config(4, 1024 * 1024)).unwrap_err(),
        LocalPackageStoreError::FetchIntentSnapshotTooLarge {
            size: MAX_FETCH_INTENT_SNAPSHOT_BYTES + 1,
            max: MAX_FETCH_INTENT_SNAPSHOT_BYTES,
        }
    );

    for root in [
        corrupt_root,
        torn_root,
        duplicate_root,
        conflicting_root,
        count_root,
        hard_count_root,
        bytes_root,
    ] {
        std::fs::remove_dir_all(root).expect("remove fetch-intent fixture");
    }
}

#[test]
fn pending_bounds_reject_the_newest_entry_without_eviction_or_cas_write() {
    assert_eq!(PENDING_CAPACITY_POLICY, PendingCapacityPolicy::RejectNewest);
    let first =
        CanonicalPackage::new(vec![PackageFile::new(b"first", b"one")]).expect("first package");
    let second =
        CanonicalPackage::new(vec![PackageFile::new(b"second", b"two")]).expect("second package");

    let count_root = temporary_store_path("count-bound");
    let mut count_store = LocalPackageStore::open(&count_root, enabled_config(1, u64::MAX))
        .expect("count-bounded store");
    count_store
        .stage(&first, "receipt:first")
        .expect("first fits");
    assert_eq!(
        count_store.stage(&second, "receipt:second"),
        Err(LocalPackageStoreError::PendingCountExceeded { max: 1 })
    );
    assert_eq!(count_store.pending().len(), 1);
    assert_eq!(count_store.pending()[0].root(), first.root());
    assert!(
        count_store.read(second.root()).is_err(),
        "a capacity rejection must happen before writing an orphan CAS object"
    );

    let bytes_root = temporary_store_path("byte-bound");
    let max_bytes = first.size_bytes() as u64 + second.size_bytes() as u64 - 1;
    let mut bytes_store = LocalPackageStore::open(&bytes_root, enabled_config(4, max_bytes))
        .expect("byte-bounded store");
    bytes_store
        .stage(&first, "receipt:first")
        .expect("first fits");
    assert_eq!(
        bytes_store.stage(&second, "receipt:second"),
        Err(LocalPackageStoreError::PendingBytesExceeded { max: max_bytes })
    );
    assert_eq!(bytes_store.pending().len(), 1);
    assert_eq!(bytes_store.pending()[0].root(), first.root());

    std::fs::remove_dir_all(count_root).expect("remove count store");
    std::fs::remove_dir_all(bytes_root).expect("remove byte store");
}

#[test]
fn recovery_keeps_the_stable_snapshot_and_removes_crash_residue() {
    let root = temporary_store_path("crash-recovery");
    let stable =
        CanonicalPackage::new(vec![PackageFile::new(b"stable", b"kept")]).expect("stable package");
    let orphan = CanonicalPackage::new(vec![PackageFile::new(b"orphan", b"remove")])
        .expect("orphan package");

    {
        let mut store =
            LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).expect("open store");
        store
            .stage(&stable, "receipt:stable")
            .expect("stage stable package");
    }

    let pending_temp = root.join(PACKAGE_PENDING_TEMP_FILE);
    std::fs::write(&pending_temp, b"torn pending write").expect("write crash residue");
    let objects = root.join(PACKAGE_OBJECTS_DIRECTORY);
    let object_temp = objects.join(format!("{}.pkg.tmp", stable.root().to_hex()));
    std::fs::write(&object_temp, b"torn object write").expect("write object temp residue");
    let orphan_path = objects.join(format!("{}.pkg", orphan.root().to_hex()));
    std::fs::write(&orphan_path, orphan.canonical_bytes()).expect("write unreferenced CAS object");

    let recovered = LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024))
        .expect("stable snapshot recovers instead of the torn temp");
    assert_eq!(recovered.pending().len(), 1);
    assert_eq!(recovered.pending()[0].root(), stable.root());
    assert!(!pending_temp.exists(), "pending temp must be cleaned");
    assert!(!object_temp.exists(), "CAS temp must be cleaned");
    assert!(
        !orphan_path.exists(),
        "a CAS write committed before a missing pending write is an orphan and must be collected"
    );

    std::fs::remove_dir_all(root).expect("remove recovery store");
}

#[test]
fn recovery_fails_closed_when_a_referenced_cas_object_is_corrupt() {
    let root = temporary_store_path("corrupt-object");
    let package =
        CanonicalPackage::new(vec![PackageFile::new(b"answer", b"original")]).expect("package");
    {
        let mut store =
            LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).expect("open store");
        store
            .stage(&package, "receipt:corruption-check")
            .expect("stage package");
    }

    let object_path = root
        .join(PACKAGE_OBJECTS_DIRECTORY)
        .join(format!("{}.pkg", package.root().to_hex()));
    let mut corrupted = package.canonical_bytes().to_vec();
    *corrupted.last_mut().expect("nonempty package") ^= 1;
    std::fs::write(&object_path, &corrupted).expect("simulate corrupt durable object");
    let actual = h_protocol(PACKAGE_SIDECAR_ROOT_DOMAIN, &[&corrupted]).to_hex();

    assert_eq!(
        LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).unwrap_err(),
        LocalPackageStoreError::ObjectRootMismatch {
            expected: package.root().to_hex(),
            actual,
        }
    );

    std::fs::remove_dir_all(root).expect("remove corrupt store");
}

#[test]
fn acknowledge_is_durable_and_collects_an_object_only_after_its_last_reference() {
    let root = temporary_store_path("acknowledge");
    let package =
        CanonicalPackage::new(vec![PackageFile::new(b"proof", b"shared")]).expect("package");
    {
        let mut store =
            LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).expect("open store");
        store
            .stage(&package, "receipt:first")
            .expect("stage first reference");
        store
            .stage(&package, "receipt:second")
            .expect("stage second reference");
        assert_eq!(store.pending().len(), 2);

        assert_eq!(
            store.acknowledge(package.root(), "receipt:first"),
            Ok(AcknowledgePackageOutcome::Acknowledged)
        );
        assert_eq!(store.pending().len(), 1);
        assert!(store.read(package.root()).is_ok());
        assert_eq!(
            store.acknowledge(package.root(), "receipt:missing"),
            Ok(AcknowledgePackageOutcome::NotPending)
        );
    }

    let mut reopened = LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024))
        .expect("acknowledged state recovers");
    assert_eq!(reopened.pending().len(), 1);
    assert_eq!(reopened.pending()[0].reference(), "receipt:second");
    assert_eq!(
        reopened.acknowledge(package.root(), "receipt:second"),
        Ok(AcknowledgePackageOutcome::Acknowledged)
    );
    assert!(reopened.pending().is_empty());

    let empty = LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024))
        .expect("empty durable queue recovers");
    assert!(empty.pending().is_empty());
    assert_eq!(
        empty.read(package.root()),
        Err(LocalPackageStoreError::MissingObject {
            root: package.root().to_hex()
        })
    );

    std::fs::remove_dir_all(root).expect("remove acknowledged store");
}

#[test]
fn pending_records_are_minimal_and_exact_restaging_is_idempotent() {
    let root = temporary_store_path("minimal-pending");
    let package =
        CanonicalPackage::new(vec![PackageFile::new(b"result", b"same")]).expect("package");
    let mut store =
        LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).expect("open store");
    assert_eq!(
        store.stage(&package, "receipt:idempotent"),
        Ok(StagePackageOutcome::Staged)
    );
    assert_eq!(
        store.stage(&package, "receipt:idempotent"),
        Ok(StagePackageOutcome::AlreadyPending)
    );
    assert_eq!(store.pending().len(), 1);

    let snapshot: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join(PACKAGE_PENDING_FILE)).expect("read pending snapshot"),
    )
    .expect("pending snapshot JSON");
    let entry = snapshot["entries"][0]
        .as_object()
        .expect("one pending object");
    let keys: std::collections::BTreeSet<&str> = entry.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        std::collections::BTreeSet::from(["reference", "root", "size_bytes"]),
        "pending entries must never persist package contents, answers or verdicts"
    );

    std::fs::remove_dir_all(root).expect("remove minimal store");
}

#[test]
fn recovery_rejects_a_pending_reference_whose_object_is_missing() {
    let root = temporary_store_path("missing-object");
    let package =
        CanonicalPackage::new(vec![PackageFile::new(b"result", b"lost")]).expect("package");
    {
        let mut store =
            LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).expect("open store");
        store
            .stage(&package, "receipt:missing-object")
            .expect("stage package");
    }
    std::fs::remove_file(
        root.join(PACKAGE_OBJECTS_DIRECTORY)
            .join(format!("{}.pkg", package.root().to_hex())),
    )
    .expect("simulate object loss");

    assert_eq!(
        LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).unwrap_err(),
        LocalPackageStoreError::MissingObject {
            root: package.root().to_hex()
        }
    );
    std::fs::remove_dir_all(root).expect("remove missing-object store");
}

#[test]
fn invalid_references_are_rejected_before_any_cas_write() {
    let root = temporary_store_path("invalid-reference");
    let package =
        CanonicalPackage::new(vec![PackageFile::new(b"result", b"not-written")]).expect("package");
    let mut store =
        LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).expect("open store");

    assert_eq!(
        store.stage(&package, ""),
        Err(LocalPackageStoreError::EmptyReference)
    );
    let oversized = "r".repeat(MAX_PACKAGE_REFERENCE_BYTES + 1);
    assert_eq!(
        store.stage(&package, &oversized),
        Err(LocalPackageStoreError::ReferenceTooLarge {
            size: MAX_PACKAGE_REFERENCE_BYTES + 1,
            max: MAX_PACKAGE_REFERENCE_BYTES,
        })
    );
    assert!(store.pending().is_empty());
    assert_eq!(
        store.read(package.root()),
        Err(LocalPackageStoreError::MissingObject {
            root: package.root().to_hex()
        })
    );

    std::fs::remove_dir_all(root).expect("remove invalid-reference store");
}

#[test]
fn failed_pending_commit_never_changes_memory_and_recovery_collects_its_orphan() {
    let root = temporary_store_path("failed-pending-commit");
    let first =
        CanonicalPackage::new(vec![PackageFile::new(b"first", b"kept")]).expect("first package");
    let second = CanonicalPackage::new(vec![PackageFile::new(b"second", b"not-pending")])
        .expect("second package");
    {
        let mut store =
            LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).expect("open store");
        store
            .stage(&first, "receipt:first")
            .expect("stage stable entry");

        let blocked_temp = root.join(PACKAGE_PENDING_TEMP_FILE);
        std::fs::create_dir(&blocked_temp).expect("block pending temp creation");
        assert!(store.stage(&second, "receipt:second").is_err());
        assert_eq!(store.pending().len(), 1);
        assert_eq!(store.pending()[0].root(), first.root());
        std::fs::remove_dir(&blocked_temp).expect("remove artificial blocker");
    }

    let recovered = LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024))
        .expect("stable queue recovers");
    assert_eq!(recovered.pending().len(), 1);
    assert_eq!(recovered.pending()[0].root(), first.root());
    assert_eq!(
        recovered.read(second.root()),
        Err(LocalPackageStoreError::MissingObject {
            root: second.root().to_hex()
        }),
        "the CAS object written before the failed pending commit is recovered as an orphan"
    );

    std::fs::remove_dir_all(root).expect("remove failed-commit store");
}

#[cfg(unix)]
#[test]
fn store_root_and_objects_directory_must_not_be_symlinks() {
    use std::os::unix::fs::symlink;

    let target = temporary_store_path("symlink-target");
    let root_link = temporary_store_path("symlink-root");
    std::fs::create_dir_all(&target).expect("create symlink target");
    symlink(&target, &root_link).expect("link store root");
    assert!(
        LocalPackageStore::open(&root_link, enabled_config(4, 1024 * 1024)).is_err(),
        "the store root itself must never be followed through a symlink"
    );

    std::fs::remove_file(&root_link).expect("remove root link");
    let regular_root = temporary_store_path("objects-link-root");
    let objects_target = temporary_store_path("objects-link-target");
    std::fs::create_dir_all(&regular_root).expect("create regular root");
    std::fs::create_dir_all(&objects_target).expect("create objects target");
    symlink(
        &objects_target,
        regular_root.join(PACKAGE_OBJECTS_DIRECTORY),
    )
    .expect("link objects directory");
    assert!(
        LocalPackageStore::open(&regular_root, enabled_config(4, 1024 * 1024)).is_err(),
        "the objects directory must never be followed through a symlink"
    );

    std::fs::remove_dir_all(target).expect("remove target");
    std::fs::remove_dir_all(regular_root).expect("remove regular root and link");
    std::fs::remove_dir_all(objects_target).expect("remove objects target");
}

#[cfg(unix)]
#[test]
fn recovery_never_deletes_symlinked_or_nonregular_gc_candidates() {
    use std::os::unix::fs::symlink;

    let root = temporary_store_path("gc-symlink");
    let stable =
        CanonicalPackage::new(vec![PackageFile::new(b"stable", b"kept")]).expect("stable package");
    {
        let mut store =
            LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).expect("open store");
        store
            .stage(&stable, "receipt:stable")
            .expect("stage stable package");
    }
    let objects = root.join(PACKAGE_OBJECTS_DIRECTORY);
    let outside = temporary_store_path("gc-outside-target");
    std::fs::write(&outside, b"must survive").expect("write outside target");

    let temp_link = objects.join(format!("{}.pkg.tmp", stable.root().to_hex()));
    symlink(&outside, &temp_link).expect("link temp candidate");
    assert!(
        LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).is_err(),
        "recovery must fail closed instead of unlinking a symlink temp"
    );
    assert!(temp_link.symlink_metadata().is_ok());
    assert_eq!(
        std::fs::read(&outside).expect("outside target"),
        b"must survive"
    );
    std::fs::remove_file(&temp_link).expect("remove temp link fixture");

    std::fs::create_dir(&temp_link).expect("create nonregular temp candidate");
    assert!(
        LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).is_err(),
        "recovery must fail closed instead of deleting a nonregular temp"
    );
    assert!(temp_link.is_dir(), "nonregular candidate must be preserved");
    std::fs::remove_dir(&temp_link).expect("remove nonregular temp fixture");

    let orphan_root = CanonicalPackage::new(vec![PackageFile::new(b"orphan", b"value")])
        .expect("orphan package")
        .root();
    let orphan_link = objects.join(format!("{}.pkg", orphan_root.to_hex()));
    symlink(&outside, &orphan_link).expect("link orphan candidate");
    assert!(
        LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).is_err(),
        "orphan GC must fail closed instead of unlinking a symlink object"
    );
    assert!(orphan_link.symlink_metadata().is_ok());
    assert_eq!(
        std::fs::read(&outside).expect("outside target"),
        b"must survive"
    );

    std::fs::remove_dir_all(root).expect("remove gc store");
    std::fs::remove_file(outside).expect("remove outside target");
}

#[cfg(unix)]
#[test]
fn recovery_never_follows_a_pending_snapshot_or_referenced_object_symlink() {
    use std::os::unix::fs::symlink;

    let root = temporary_store_path("authority-symlink");
    let package =
        CanonicalPackage::new(vec![PackageFile::new(b"result", b"authority")]).expect("package");
    {
        let mut store =
            LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).expect("open store");
        store
            .stage(&package, "receipt:authority")
            .expect("stage package");
    }

    let pending = root.join(PACKAGE_PENDING_FILE);
    let pending_outside = temporary_store_path("pending-outside");
    std::fs::rename(&pending, &pending_outside).expect("move pending snapshot");
    symlink(&pending_outside, &pending).expect("link pending snapshot");
    assert!(
        LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).is_err(),
        "pending authority must be opened O_NOFOLLOW"
    );
    std::fs::remove_file(&pending).expect("remove pending link");
    std::fs::rename(&pending_outside, &pending).expect("restore pending snapshot");

    let object = root
        .join(PACKAGE_OBJECTS_DIRECTORY)
        .join(format!("{}.pkg", package.root().to_hex()));
    let object_outside = temporary_store_path("object-outside");
    std::fs::rename(&object, &object_outside).expect("move object");
    symlink(&object_outside, &object).expect("link object");
    assert!(
        LocalPackageStore::open(&root, enabled_config(4, 1024 * 1024)).is_err(),
        "referenced CAS authority must be opened O_NOFOLLOW"
    );
    assert!(
        object.symlink_metadata().is_ok(),
        "link must not be deleted"
    );

    std::fs::remove_dir_all(root).expect("remove symlink store");
    std::fs::remove_file(object_outside).expect("remove outside object");
}

#[test]
fn recovery_rejects_oversized_files_from_metadata_before_parsing_or_hashing() {
    let pending_root = temporary_store_path("oversized-pending");
    LocalPackageStore::open(&pending_root, enabled_config(4, 1024 * 1024))
        .expect("initialize pending store");
    let pending_path = pending_root.join(PACKAGE_PENDING_FILE);
    let pending_file = std::fs::File::create(&pending_path).expect("create sparse pending file");
    pending_file
        .set_len(MAX_PENDING_SNAPSHOT_BYTES + 1)
        .expect("size sparse pending file");
    assert_eq!(
        LocalPackageStore::open(&pending_root, enabled_config(4, 1024 * 1024)).unwrap_err(),
        LocalPackageStoreError::PendingSnapshotTooLarge {
            size: MAX_PENDING_SNAPSHOT_BYTES + 1,
            max: MAX_PENDING_SNAPSHOT_BYTES,
        }
    );

    let object_root = temporary_store_path("oversized-object");
    let package =
        CanonicalPackage::new(vec![PackageFile::new(b"result", b"small")]).expect("package");
    {
        let mut store = LocalPackageStore::open(&object_root, enabled_config(4, 1024 * 1024))
            .expect("open object store");
        store
            .stage(&package, "receipt:oversized-object")
            .expect("stage package");
    }
    let object_path = object_root
        .join(PACKAGE_OBJECTS_DIRECTORY)
        .join(format!("{}.pkg", package.root().to_hex()));
    let object_file = std::fs::OpenOptions::new()
        .write(true)
        .open(&object_path)
        .expect("open sparse object");
    object_file
        .set_len(boole_core::MAX_PACKAGE_CANONICAL_BYTES as u64 + 1)
        .expect("size sparse object");
    assert_eq!(
        LocalPackageStore::open(&object_root, enabled_config(4, 1024 * 1024)).unwrap_err(),
        LocalPackageStoreError::ObjectTooLarge {
            size: boole_core::MAX_PACKAGE_CANONICAL_BYTES as u64 + 1,
            max: boole_core::MAX_PACKAGE_CANONICAL_BYTES as u64,
        }
    );

    std::fs::remove_dir_all(pending_root).expect("remove pending store");
    std::fs::remove_dir_all(object_root).expect("remove object store");
}

#[cfg(unix)]
#[test]
fn store_authority_pins_a_resolved_parent_and_never_creates_missing_parents() {
    use std::os::unix::fs::symlink;

    let base = temporary_store_path("strict-parent");
    let real_parent = base.join("real-parent");
    let replacement_parent = base.join("replacement-parent");
    let linked_parent = base.join("linked-parent");
    std::fs::create_dir_all(&real_parent).expect("create real parent");
    std::fs::create_dir_all(replacement_parent.join("store/objects"))
        .expect("create replacement store-shaped tree");
    symlink(&real_parent, &linked_parent).expect("link parent");
    let linked_store = linked_parent.join("store");
    let mut store = LocalPackageStore::open(&linked_store, enabled_config(4, 1024 * 1024))
        .expect("resolve and retain the already-existing real parent");
    std::fs::remove_file(&linked_parent).expect("remove original parent link");
    symlink(&replacement_parent, &linked_parent).expect("retarget parent link after open");
    let package = CanonicalPackage::new(vec![PackageFile::new(b"answer", b"parent-pinned")])
        .expect("package");
    store
        .stage(&package, "receipt:parent-pinned")
        .expect("write through retained authority");
    assert!(
        real_parent
            .join("store/objects")
            .join(format!("{}.pkg", package.root().to_hex()))
            .is_file(),
        "resolved real parent remains the authority after path retargeting"
    );
    assert!(
        !replacement_parent
            .join("store/objects")
            .join(format!("{}.pkg", package.root().to_hex()))
            .exists(),
        "retargeted ancestor must not redirect writes"
    );
    assert!(
        real_parent
            .join("store")
            .join(PACKAGE_PENDING_FILE)
            .is_file(),
        "pending authority must stay in the originally resolved root"
    );
    assert!(
        !replacement_parent
            .join("store")
            .join(PACKAGE_PENDING_FILE)
            .exists(),
        "retargeted ancestor must not redirect the pending snapshot"
    );

    let missing_parent_store = base.join("missing-a").join("missing-b");
    assert!(
        LocalPackageStore::open(&missing_parent_store, enabled_config(4, 1024 * 1024)).is_err()
    );
    assert!(
        !base.join("missing-a").exists(),
        "open may create only the final store root, never missing ancestors"
    );

    std::fs::remove_dir_all(base).expect("remove strict-parent fixtures");
}

#[cfg(unix)]
#[test]
fn opened_relative_store_and_objects_authority_survive_path_replacement() {
    use std::os::unix::fs::symlink;

    let _cwd_guard = current_directory_test_lock()
        .lock()
        .expect("current-directory test lock");
    let original_cwd = std::env::current_dir().expect("read original cwd");
    let base = temporary_store_path("relative-authority");
    let first_cwd = base.join("first");
    let second_cwd = base.join("second");
    let outside_objects = base.join("outside-objects");
    std::fs::create_dir_all(&first_cwd).expect("create first cwd");
    std::fs::create_dir_all(&second_cwd).expect("create second cwd");
    std::fs::create_dir_all(&outside_objects).expect("create outside objects");

    let result = std::panic::catch_unwind(|| {
        std::env::set_current_dir(&first_cwd).expect("enter first cwd");
        let mut store = LocalPackageStore::open("store", enabled_config(4, 1024 * 1024))
            .expect("open one-component relative store");

        let store_root = first_cwd.join("store");
        let retained_objects = store_root.join("objects-retained");
        std::fs::rename(
            store_root.join(PACKAGE_OBJECTS_DIRECTORY),
            &retained_objects,
        )
        .expect("move opened objects directory");
        symlink(&outside_objects, store_root.join(PACKAGE_OBJECTS_DIRECTORY))
            .expect("replace literal objects path");
        std::env::set_current_dir(&second_cwd).expect("move process cwd after open");

        let package =
            CanonicalPackage::new(vec![PackageFile::new(b"answer", b"anchored")]).expect("package");
        store
            .register_fetch_intents(&[(package.root(), "receipt:anchored".to_owned())])
            .expect("fetch intent stays relative to retained root authority");
        assert!(
            store_root.join(PACKAGE_FETCH_INTENTS_FILE).is_file(),
            "fetch-intent snapshot must land in the retained store root"
        );
        assert!(
            !second_cwd
                .join("store")
                .join(PACKAGE_FETCH_INTENTS_FILE)
                .exists(),
            "changing cwd must not redirect a fetch-intent snapshot"
        );
        store
            .stage(&package, "receipt:anchored")
            .expect("all operations stay relative to retained directory descriptors");
        assert_eq!(
            store
                .complete_fetch_intent(package.root(), "receipt:anchored")
                .expect("fetch-intent cleanup stays relative to retained root authority"),
            CompletePackageFetchIntentOutcome::Completed
        );
        assert!(store.fetch_intents().is_empty());
        assert_eq!(
            store.read(package.root()).expect("read anchored object"),
            package.canonical_bytes()
        );
        assert!(
            retained_objects
                .join(format!("{}.pkg", package.root().to_hex()))
                .is_file(),
            "CAS write must land in the retained objects directory"
        );
        assert_eq!(
            std::fs::read_dir(&outside_objects)
                .expect("scan outside objects")
                .count(),
            0,
            "literal path replacement must not redirect a CAS write"
        );
        assert_eq!(
            store
                .acknowledge(package.root(), "receipt:anchored")
                .expect("acknowledge through retained authority"),
            AcknowledgePackageOutcome::Acknowledged
        );
        assert!(
            !retained_objects
                .join(format!("{}.pkg", package.root().to_hex()))
                .exists(),
            "CAS deletion must stay inside the retained objects directory"
        );
        assert_eq!(
            std::fs::read_dir(&outside_objects)
                .expect("rescan outside objects")
                .count(),
            0,
            "literal path replacement must not redirect a CAS deletion"
        );
    });

    std::env::set_current_dir(&original_cwd).expect("restore original cwd");
    std::fs::remove_dir_all(&base).expect("remove relative-authority fixtures");
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}

#[test]
fn pending_metadata_caps_and_same_root_size_conflicts_precede_any_cas_read() {
    let over_cap_root = temporary_store_path("pending-metadata-cap-first");
    LocalPackageStore::open(&over_cap_root, enabled_config(4, 1024 * 1024))
        .expect("initialize store");
    let package =
        CanonicalPackage::new(vec![PackageFile::new(b"answer", b"not-present")]).expect("package");
    let size = package.size_bytes() as u64;
    let over_cap = serde_json::json!({
        "schema": "boole.useful-work.package-pending.v1",
        "entries": [
            {"root": package.root().to_hex(), "size_bytes": size, "reference": "receipt:one"},
            {"root": package.root().to_hex(), "size_bytes": size, "reference": "receipt:two"}
        ]
    });
    std::fs::write(
        over_cap_root.join(PACKAGE_PENDING_FILE),
        serde_json::to_vec(&over_cap).expect("serialize over-cap snapshot"),
    )
    .expect("write over-cap snapshot");
    assert_eq!(
        LocalPackageStore::open(
            &over_cap_root,
            enabled_config(4, size.saturating_mul(2).saturating_sub(1)),
        )
        .unwrap_err(),
        LocalPackageStoreError::Corrupt("pending bytes exceed configured bound".into()),
        "aggregate metadata must reject before trying to read the absent CAS object"
    );

    let conflicting_root = temporary_store_path("pending-root-size-conflict");
    LocalPackageStore::open(&conflicting_root, enabled_config(4, 1024 * 1024))
        .expect("initialize conflicting store");
    let conflicting = serde_json::json!({
        "schema": "boole.useful-work.package-pending.v1",
        "entries": [
            {"root": package.root().to_hex(), "size_bytes": size, "reference": "receipt:one"},
            {"root": package.root().to_hex(), "size_bytes": size + 1, "reference": "receipt:two"}
        ]
    });
    std::fs::write(
        conflicting_root.join(PACKAGE_PENDING_FILE),
        serde_json::to_vec(&conflicting).expect("serialize conflicting snapshot"),
    )
    .expect("write conflicting snapshot");
    assert_eq!(
        LocalPackageStore::open(&conflicting_root, enabled_config(4, 1024 * 1024)).unwrap_err(),
        LocalPackageStoreError::Corrupt("conflicting sizes for same pending root".into()),
        "same-root metadata conflict must reject before trying to read the absent CAS object"
    );

    std::fs::remove_dir_all(over_cap_root).expect("remove over-cap store");
    std::fs::remove_dir_all(conflicting_root).expect("remove conflicting store");
}
