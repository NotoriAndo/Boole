use boole_core::{CanonicalPackage, PackageFile};
use boole_core::{LocalPackageStore, LocalPackageStoreConfig};
use boole_native_rust_meter::tuple_verifier::{verify, Verdict};
use boole_node::metered_tuple_package::restore_package;
use boole_node::metered_tuple_package::PackageError;
use boole_node::metered_tuple_package::{build_package, verify_package};
use boole_node::metered_tuple_package::{import_package, reverify_stored, StoredVerification};

const TASK: &[u8] = include_bytes!("../../../fixtures/native-metered-tuple-v1/task.json");
const ANSWER: &[u8] = include_bytes!("../../../fixtures/native-metered-tuple-v1/answer.rs");

#[test]
fn complete_package_reproduces_the_actual_verifier_result() {
    let package = build_package(TASK, ANSWER).unwrap();
    let result = verify_package(&package, package.root()).unwrap();
    assert_eq!(result.verdict(), Verdict::Accepted);
    assert_eq!(
        result.canonical_bytes(),
        verify(TASK, ANSWER).unwrap().canonical_bytes()
    );
}

#[test]
fn missing_package_is_durable_unavailability_then_reverified_after_restart() {
    let root_dir = std::env::temp_dir().join(format!(
        "boole-metered-package-{}-{}",
        std::process::id(),
        boole_testkit::rand_suffix()
    ));
    let config = LocalPackageStoreConfig {
        enabled: true,
        ..Default::default()
    };
    let package = build_package(TASK, ANSWER).unwrap();
    let root = package.root();
    {
        let mut store = LocalPackageStore::open(&root_dir, config.clone()).unwrap();
        assert!(matches!(
            reverify_stored(&mut store, root).unwrap(),
            StoredVerification::RetryableUnavailable
        ));
        assert_eq!(store.fetch_intents().len(), 1);
        assert!(store.pending().is_empty());
    }
    {
        let mut store = LocalPackageStore::open(&root_dir, config.clone()).unwrap();
        assert_eq!(store.fetch_intents()[0].root(), root);
        import_package(&mut store, root, package.canonical_bytes()).unwrap();
        assert!(store.fetch_intents().is_empty());
    }
    let mut store = LocalPackageStore::open(&root_dir, config).unwrap();
    let StoredVerification::Verified(result) = reverify_stored(&mut store, root).unwrap() else {
        panic!("recovered bytes must actually be verified");
    };
    assert_eq!(
        result.canonical_bytes(),
        verify(TASK, ANSWER).unwrap().canonical_bytes()
    );
    assert_eq!(
        store.pending().len(),
        1,
        "verification must not destroy DA material"
    );
    drop(store);
    std::fs::remove_dir_all(root_dir).unwrap();
}

#[test]
fn package_cannot_choose_its_implementation_budget_or_claimed_verdict() {
    let package = build_package(TASK, ANSWER).unwrap();
    let contract: serde_json::Value = serde_json::from_slice(
        package
            .files()
            .find(|(path, _)| *path == b"verifier.json")
            .unwrap()
            .1,
    )
    .unwrap();
    for field in [
        "adapter",
        "implementationDigest",
        "policyDigest",
        "limits",
        "nonIssuable",
        "activationAllowed",
    ] {
        let mut changed = contract.clone();
        changed[field] = serde_json::json!("overridden");
        let tampered = CanonicalPackage::new(vec![
            PackageFile::new("task.json", TASK),
            PackageFile::new("answer.rs", ANSWER),
            PackageFile::new("verifier.json", serde_json::to_vec(&changed).unwrap()),
        ])
        .unwrap();
        assert!(
            matches!(
                verify_package(&tampered, tampered.root()),
                Err(PackageError::ContractMismatch)
            ),
            "{field}"
        );
    }
    let mut files = package
        .files()
        .map(|(p, b)| PackageFile::new(p, b))
        .collect::<Vec<_>>();
    files.push(PackageFile::new(
        "claimed-verdict.json",
        br#"{"verdict":"accepted"}"#,
    ));
    let extra = CanonicalPackage::new(files).unwrap();
    assert!(matches!(
        verify_package(&extra, extra.root()),
        Err(PackageError::InvalidLayout)
    ));
    let missing = CanonicalPackage::new(vec![
        PackageFile::new("answer.rs", ANSWER),
        PackageFile::new("task.json", TASK),
    ])
    .unwrap();
    assert!(matches!(
        verify_package(&missing, missing.root()),
        Err(PackageError::InvalidLayout)
    ));
}

#[test]
fn bound_wrong_answer_is_rejected_but_root_substitution_is_not_an_answer_verdict() {
    let correct = build_package(TASK, ANSWER).unwrap();
    let wrong = build_package(TASK, b"7").unwrap();
    assert!(matches!(
        verify_package(&wrong, correct.root()),
        Err(PackageError::RootMismatch)
    ));
    let result = verify_package(&wrong, wrong.root()).unwrap();
    assert_eq!(result.verdict(), Verdict::DeterministicReject);
    assert_eq!(result.reason(), "answer_mismatch");
    assert_eq!(
        result.canonical_bytes(),
        verify(TASK, b"7").unwrap().canonical_bytes()
    );
}

#[test]
fn deleted_staged_bytes_require_exact_offline_restore_before_reverification() {
    let path = std::env::temp_dir().join(format!(
        "boole-metered-restore-{}-{}",
        std::process::id(),
        boole_testkit::rand_suffix()
    ));
    let config = LocalPackageStoreConfig {
        enabled: true,
        ..Default::default()
    };
    let package = build_package(TASK, ANSWER).unwrap();
    let root = package.root();
    let mut store = LocalPackageStore::open(&path, config.clone()).unwrap();
    import_package(&mut store, root, package.canonical_bytes()).unwrap();
    drop(store);
    let pending_path = path.join(boole_core::PACKAGE_PENDING_FILE);
    let pending = std::fs::read(&pending_path).unwrap();
    let object = path
        .join(boole_core::PACKAGE_OBJECTS_DIRECTORY)
        .join(format!("{}.pkg", root.to_hex()));
    std::fs::remove_file(&object).unwrap();
    assert!(matches!(
        LocalPackageStore::open(&path, config.clone()),
        Err(boole_core::LocalPackageStoreError::MissingObject { .. })
    ));
    let wrong = build_package(TASK, b"7").unwrap();
    assert!(restore_package(&path, config.clone(), root, wrong.canonical_bytes()).is_err());
    assert!(!object.exists());
    restore_package(&path, config.clone(), root, package.canonical_bytes()).unwrap();
    assert_eq!(std::fs::read(&pending_path).unwrap(), pending);
    let mut reopened = LocalPackageStore::open(&path, config).unwrap();
    let StoredVerification::Verified(result) = reverify_stored(&mut reopened, root).unwrap() else {
        panic!("restored package unavailable");
    };
    assert_eq!(
        result.canonical_bytes(),
        verify(TASK, ANSWER).unwrap().canonical_bytes()
    );
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn failed_import_and_pending_backpressure_never_become_verdicts_or_drop_intents() {
    let path = std::env::temp_dir().join(format!(
        "boole-metered-backpressure-{}-{}",
        std::process::id(),
        boole_testkit::rand_suffix()
    ));
    let config = LocalPackageStoreConfig {
        enabled: true,
        max_pending_packages: 1,
        max_pending_bytes: 64 * 1024,
    };
    let mut store = LocalPackageStore::open(&path, config).unwrap();
    let package = build_package(TASK, ANSWER).unwrap();
    let wrong = build_package(TASK, b"7").unwrap();
    assert!(matches!(
        reverify_stored(&mut store, package.root()).unwrap(),
        StoredVerification::RetryableUnavailable
    ));
    assert!(import_package(&mut store, package.root(), wrong.canonical_bytes()).is_err());
    assert!(matches!(
        reverify_stored(&mut store, wrong.root()),
        Err(boole_node::metered_tuple_package::ReverifyError::Store(
            boole_core::LocalPackageStoreError::FetchIntentCountExceeded { max: 1 }
        ))
    ));
    assert!(store.pending().is_empty());
    assert_eq!(store.fetch_intents().len(), 1);
    assert_eq!(store.fetch_intents()[0].root(), package.root());
    import_package(&mut store, package.root(), package.canonical_bytes()).unwrap();
    assert!(store.fetch_intents().is_empty());
    assert_eq!(store.pending().len(), 1);
    drop(store);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn maximum_verifier_inputs_fit_but_oversized_packages_are_not_parsed_or_executed() {
    let mut task = TASK.to_vec();
    task.resize(
        boole_native_rust_meter::tuple_verifier::MAX_TASK_BYTES,
        b' ',
    );
    let mut answer = ANSWER.to_vec();
    answer.resize(
        boole_native_rust_meter::tuple_verifier::MAX_ANSWER_BYTES,
        b' ',
    );
    let package = build_package(&task, &answer).unwrap();
    assert!(package.size_bytes() <= boole_node::metered_tuple_package::MAX_METERED_PACKAGE_BYTES);
    assert_eq!(
        verify_package(&package, package.root()).unwrap().verdict(),
        Verdict::Accepted
    );
    let oversized =
        CanonicalPackage::new(vec![PackageFile::new("answer.rs", vec![0; 16 * 1024])]).unwrap();
    assert!(matches!(
        verify_package(&oversized, oversized.root()),
        Err(PackageError::TooLarge)
    ));
}
