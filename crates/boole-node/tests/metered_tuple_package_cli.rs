use boole_core::CanonicalPackage;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boole-metered-package-cli-{}-{}",
            std::process::id(),
            boole_testkit::rand_suffix()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/native-metered-tuple-v1")
        .join(name)
}

fn run(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_boole-metered-tuple-package"))
        .args(args)
        .env_clear()
        .env("BOOLE_MAX_FUEL", "0")
        .env("RUSTFLAGS", "--invalid-do-not-execute-a-compiler")
        .output()
        .unwrap()
}

#[test]
fn independent_processes_read_two_stores_and_reproduce_the_golden_verdict() {
    let scratch = Scratch::new();
    let packed = run(&[
        "pack".as_ref(),
        fixture("task.json").as_os_str(),
        fixture("answer.rs").as_os_str(),
    ]);
    assert!(packed.status.success(), "{packed:?}");
    let package = CanonicalPackage::from_canonical_bytes(&packed.stdout).unwrap();
    let root = package.root().to_hex();
    let file = scratch.0.join("package.bin");
    std::fs::write(&file, &packed.stdout).unwrap();
    let corpus: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("corpus.json")).unwrap()).unwrap();
    let mut previous = None;
    for label in ["first", "second"] {
        let store = scratch.0.join(label);
        let imported = run(&[
            "import".as_ref(),
            store.as_os_str(),
            root.as_ref(),
            file.as_os_str(),
        ]);
        assert!(imported.status.success(), "{imported:?}");
        for _ in 0..2 {
            let checked = run(&["verify".as_ref(), store.as_os_str(), root.as_ref()]);
            assert!(checked.status.success(), "{checked:?}");
            let value: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
            assert_eq!(value["packageRoot"], root);
            assert_eq!(value["verification"], corpus["cases"][0]["expected"]);
            if let Some(bytes) = &previous {
                assert_eq!(&checked.stdout, bytes);
            }
            previous = Some(checked.stdout);
        }
    }
}

#[test]
fn process_restart_preserves_missing_request_and_offline_restore_preserves_pending() {
    let scratch = Scratch::new();
    let packed = run(&[
        "pack".as_ref(),
        fixture("task.json").as_os_str(),
        fixture("answer.rs").as_os_str(),
    ]);
    assert!(packed.status.success());
    let root = CanonicalPackage::from_canonical_bytes(&packed.stdout)
        .unwrap()
        .root()
        .to_hex();
    let file = scratch.0.join("package.bin");
    std::fs::write(&file, packed.stdout).unwrap();
    let store = scratch.0.join("store");
    for _ in 0..2 {
        let missing = run(&["verify".as_ref(), store.as_os_str(), root.as_ref()]);
        assert_eq!(missing.status.code(), Some(2));
        let value: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
        assert_eq!(value["outcome"], "retryable_unavailable");
        assert!(value.get("verification").is_none());
    }
    let imported = run(&[
        "import".as_ref(),
        store.as_os_str(),
        root.as_ref(),
        file.as_os_str(),
    ]);
    assert!(imported.status.success());
    let before = run(&["verify".as_ref(), store.as_os_str(), root.as_ref()]);
    assert!(before.status.success());
    let pending = std::fs::read(store.join(boole_core::PACKAGE_PENDING_FILE)).unwrap();
    let object = store
        .join(boole_core::PACKAGE_OBJECTS_DIRECTORY)
        .join(format!("{root}.pkg"));
    std::fs::remove_file(&object).unwrap();
    let lost = run(&["verify".as_ref(), store.as_os_str(), root.as_ref()]);
    assert_eq!(lost.status.code(), Some(2));
    let restored = run(&[
        "restore".as_ref(),
        store.as_os_str(),
        root.as_ref(),
        file.as_os_str(),
    ]);
    assert!(restored.status.success(), "{restored:?}");
    assert_eq!(
        std::fs::read(store.join(boole_core::PACKAGE_PENDING_FILE)).unwrap(),
        pending
    );
    let after = run(&["verify".as_ref(), store.as_os_str(), root.as_ref()]);
    assert_eq!(after.status.code(), Some(0));
    assert_eq!(after.stdout, before.stdout);
    std::fs::write(&object, b"corrupt-present-evidence").unwrap();
    let refused = run(&[
        "restore".as_ref(),
        store.as_os_str(),
        root.as_ref(),
        file.as_os_str(),
    ]);
    assert_eq!(refused.status.code(), Some(2));
    assert_eq!(std::fs::read(object).unwrap(), b"corrupt-present-evidence");
}

#[test]
fn malformed_oversized_and_nonregular_files_never_produce_verification_or_stage_bytes() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new();
    let oversized = scratch.0.join("large");
    let link = scratch.0.join("link");
    let fifo = scratch.0.join("fifo");
    let store = scratch.0.join("store");
    let root = "00".repeat(32);
    std::fs::write(&oversized, vec![0u8; 16 * 1024 + 1]).unwrap();
    symlink(&oversized, &link).unwrap();
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    for file in [&oversized, &link, &fifo, &scratch.0] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_boole-metered-tuple-package"))
            .arg("import")
            .arg(&store)
            .arg(&root)
            .arg(file)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("nonregular package input blocked");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(value.get("verification").is_none());
        assert!(!store.exists(), "refused file input must not open a store");
    }
    let bad = scratch.0.join("malformed");
    std::fs::write(&bad, b"not-a-canonical-package").unwrap();
    let refused = run(&[
        "import".as_ref(),
        store.as_os_str(),
        root.as_ref(),
        bad.as_os_str(),
    ]);
    assert_eq!(refused.status.code(), Some(2));
    let value: serde_json::Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(value["outcome"], "package_error");
    assert!(value.get("verification").is_none());
    let reopened = boole_core::LocalPackageStore::open(
        &store,
        boole_core::LocalPackageStoreConfig {
            enabled: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(reopened.pending().is_empty());
    assert!(reopened.fetch_intents().is_empty());
}
