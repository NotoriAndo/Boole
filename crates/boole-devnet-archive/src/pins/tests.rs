use super::*;
use crate::{durability::PrivateTempDir, input::empty_fixture};
use boole_derivation::{m2::Config, ParentHeader};
struct Fixture {
    _dir: PrivateTempDir,
    source: InputJournal,
    pins: PinStore,
    request: CandidatePin,
    artifacts: Vec<ArtifactSource>,
}
fn binding(source: &InputJournal, h: u64) -> Result<HeaderBinding> {
    let l1 = source.get(h)?.block;
    let root = [9; 32];
    let hash = if h == 0 {
        source.network().l2_genesis_hash.clone()
    } else {
        hex_string(&super::hash(&h.to_be_bytes()))
    };
    let parent = if h == 1 {
        source.network().l2_genesis_hash.clone()
    } else {
        hex_string(&super::hash(&h.saturating_sub(1).to_be_bytes()))
    };
    let transactions = if h == 0 {
        vec![]
    } else {
        vec![
            boole_derivation::system::l1_info(&l1)?,
            boole_derivation::system::m2_root_deposit(&l1, root)?,
        ]
    };
    Ok(HeaderBinding {
        header: ParentHeader {
            number: h,
            hash,
            parent_hash: (h > 0).then_some(parent),
            timestamp: h,
            gas_limit: 30_000_000,
            miner: hex_string(&[0; 20]),
            extra_data: "0x".into(),
            l1_info: transactions.first().cloned(),
            m2_root_deposit: transactions.get(1).cloned(),
        },
        transactions,
        root,
    })
}
fn fixture(complete: bool) -> Result<Fixture> {
    let dir = PrivateTempDir::new("m4c2-pins")?;
    let network = NetworkIdentity::new(&Config::devnet(), None, hex_string(&[1; 32]), 30_000_000)?;
    let mut source = InputJournal::open(
        &dir.path().join("node"),
        network,
        || Ok(empty_fixture(0, [0; 32])),
        false,
    )?;
    for h in 1..=4 {
        let parent = source.get(h - 1)?.block.hash;
        source.append(&empty_fixture(
            h,
            boole_derivation::attributes::hex_bytes(&parent, "parent")?
                .try_into()
                .unwrap(),
        ))?;
    }
    let bin_hash = super::hash(b"pinned fixture binary");
    let descriptor = serde_json::to_vec(
        &serde_json::json!({"adapter":"boole-devnet-stub-v1","binaries":{"fixture":bin_hash}}),
    )?;
    let descriptor = descriptor.as_slice();
    let gate = super::hash(&[b"BOOLE-M3/GATE-DESCRIPTOR/v1".as_slice(), descriptor].concat());
    let bodies: Vec<(_, &[u8], bool)> = vec![
        (
            ArtifactRole::StubDescriptor { epoch: 0, gate },
            descriptor,
            false,
        ),
        (
            ArtifactRole::StubBinary {
                epoch: 0,
                gate,
                profile: "fixture".into(),
            },
            b"pinned fixture binary",
            true,
        ),
        (
            ArtifactRole::Environment {
                component: super::hash(b"pinned environment"),
            },
            b"pinned environment",
            false,
        ),
    ];
    let mut artifacts = vec![];
    for (n, (role, bytes, executable)) in bodies.into_iter().enumerate() {
        let path = dir.path().join(format!("source-{n}"));
        std::fs::write(&path, bytes)?;
        artifacts.push(ArtifactSource {
            role,
            digest: super::hash(bytes),
            path,
            executable,
        });
    }
    let request = CandidatePin {
        key: ([1; 32], [2; 32]),
        reasons: [super::super::Reason::Accepted].into(),
        commit_height: 1,
        open_height: 2,
        completion_height: 4,
        reveal_height: 1,
        gates: [(0, gate)].into(),
        components: artifacts
            .iter()
            .filter_map(|a| match a.role {
                ArtifactRole::Environment { component } => Some(component),
                _ => None,
            })
            .collect(),
    };
    let mut pins = PinStore::open(&source)?;
    pins.install_artifacts(&artifacts)?;
    if complete {
        for h in 0..=4 {
            let b = binding(&source, h)?;
            let hash = b.header.hash.clone();
            pins.prepare(
                &source,
                b,
                if h == 4 {
                    std::slice::from_ref(&request)
                } else {
                    &[]
                },
            )?;
            pins.select(h, &hash, [9; 32])?;
        }
    }
    Ok(Fixture {
        _dir: dir,
        source,
        pins,
        request,
        artifacts,
    })
}
#[test]
fn t88_deleted_old_gate_artifact_is_availability_without_original_source_fallback() -> Result<()> {
    let mut f = fixture(true)?;
    f.pins
        .view()
        .verify(&f.source, std::slice::from_ref(&f.request), 4)?;
    let artifact = &f.artifacts[1];
    std::fs::remove_file(f.pins.view().artifact_path(artifact.digest))?;
    let missing = f
        .pins
        .view()
        .verify(&f.source, std::slice::from_ref(&f.request), 4);
    assert!(missing.is_err(), "durable pin deletion was accepted");
    assert!(
        f.pins.install_artifacts(&f.artifacts).is_err(),
        "missing pinned bytes silently reacquired from original source"
    );
    Ok(())
}

#[test]
fn t88_omitted_interior_part_height_is_not_durable_coverage() -> Result<()> {
    let mut f = fixture(false)?;
    for h in 0..=4 {
        let b = binding(&f.source, h)?;
        let hash = b.header.hash.clone();
        f.pins.prepare(&f.source, b, &[])?;
        f.pins.select(h, &hash, [9; 32])?;
    }
    // A validly encoded old/buggy writer's claim omits height 3. Independent
    // read verification must check the whole PART interval, not its endpoints.
    let inputs = [1, 2, 4]
        .into_iter()
        .map(|h| Ok((h, row_hash(&f.source.row(h)?)?)))
        .collect::<Result<Vec<_>>>()?;
    f.pins.append(Event::Prepared {
        binding: Box::new(binding(&f.source, 4)?),
        candidates: vec![f.request.clone()],
        inputs,
    })?;
    let view =
        ReadOnlyPins::open(f.source.root(), f.source.network(), f.source.anchor_hash())?.unwrap();
    assert!(
        view.verify(&f.source, std::slice::from_ref(&f.request), 4)
            .is_err(),
        "endpoint-only pin set falsely covered an interior PART height"
    );
    Ok(())
}

#[test]
fn t88_missing_l2_binding_blocks_the_dependent_selection() -> Result<()> {
    let mut f = fixture(false)?;
    for h in [0, 1, 2] {
        let b = binding(&f.source, h)?;
        let hash = b.header.hash.clone();
        f.pins.prepare(&f.source, b, &[])?;
        f.pins.select(h, &hash, [9; 32])?;
    }
    let b = binding(&f.source, 4)?;
    let hash = b.header.hash.clone();
    let result = f
        .pins
        .prepare(&f.source, b, std::slice::from_ref(&f.request))
        .and_then(|_| f.pins.select(4, &hash, [9; 32]));
    assert!(
        result.is_err(),
        "selected dependent state despite the missing interior L2 binding"
    );
    assert_eq!(f.pins.view().height(), 2);
    Ok(())
}

#[test]
fn t81_concurrent_artifact_change_cannot_leave_the_read_only_view_ok() -> Result<()> {
    let f = fixture(true)?;
    let view =
        ReadOnlyPins::open(f.source.root(), f.source.network(), f.source.anchor_hash())?.unwrap();
    view.verify(&f.source, std::slice::from_ref(&f.request), 4)?;
    let p = view.artifact_path(f.artifacts[2].digest);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::write(&p, b"changed environment")?;
    assert!(
        view.check().is_err(),
        "concurrent pinned artifact change left the view valid"
    );
    Ok(())
}

#[test]
fn t88_foreign_manifest_is_refused_and_source_is_preserved() -> Result<()> {
    let f = fixture(true)?;
    let p = f.source.root().join(DIR).join(MANIFEST);
    let mut m: Manifest = serde_json::from_slice(&std::fs::read(&p)?)?;
    m.network.l2_chain_id += 1;
    let bytes = serde_json::to_vec(&m)?;
    std::fs::write(&p, &bytes)?;
    assert!(
        ReadOnlyPins::open(f.source.root(), f.source.network(), f.source.anchor_hash()).is_err()
    );
    assert_eq!(std::fs::read(&p)?, bytes);
    assert!(!f.source.root().join(DIR).join("corrupt").exists());
    Ok(())
}
#[test]
fn t81_read_only_source_does_not_take_the_held_writer_lock() -> Result<()> {
    if let Some(root) = std::env::var_os("BOOLE_PIN_READ_CHILD") {
        let network: NetworkIdentity =
            serde_json::from_str(&std::env::var("BOOLE_PIN_READ_NETWORK")?)?;
        let req: CandidatePin = serde_json::from_str(&std::env::var("BOOLE_PIN_READ_REQUEST")?)?;
        let source = ReadOnlyJournal::open(Path::new(&root), &network)?;
        let view = ReadOnlyPins::open(Path::new(&root), &network, source.anchor_hash())?.unwrap();
        return view.verify(&source, &[req], 4);
    }
    let f = fixture(true)?;
    let files = f.pins.view.files.clone();
    let out = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "pins::storage::tests::t81_read_only_source_does_not_take_the_held_writer_lock",
            "--nocapture",
        ])
        .env("BOOLE_PIN_READ_CHILD", f.source.root())
        .env(
            "BOOLE_PIN_READ_NETWORK",
            serde_json::to_string(f.source.network())?,
        )
        .env("BOOLE_PIN_READ_REQUEST", serde_json::to_string(&f.request)?)
        .output()?;
    assert!(
        out.status.success(),
        "read-only child failed under writer lock: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(f.pins.view.files, files);
    f.pins.view.check()?;
    Ok(())
}
#[test]
fn t88_restart_rebuilds_the_complete_union_and_pins() -> Result<()> {
    let f = fixture(true)?;
    let root = f.source.root().to_path_buf();
    let network = f.source.network().clone();
    let req = f.request.clone();
    drop(f.pins);
    let source = ReadOnlyJournal::open(&root, &network)?;
    let view = ReadOnlyPins::open(&root, &network, source.anchor_hash())?.unwrap();
    assert!(view.intervals().contains(3));
    assert_eq!(view.intervals().0.len(), 1);
    view.verify(&source, &[req], 4)?;
    let a = view.anchor(4)?;
    view.verify_anchor(&a)?;
    Ok(())
}

#[test]
fn t88_checkpoint_anchor_cannot_borrow_a_selection_from_a_later_suffix() -> Result<()> {
    let f = fixture(true)?;
    let mut a = f.pins.view().anchor(4)?;
    let (raw, _) = read_bound(
        &f.source
            .root()
            .join(DIR)
            .join("records")
            .join(&f.pins.view.files[0].0),
        RECORD_MAX,
    )?;
    // The first record installs artifacts, before any selected L2 binding.
    a.record = frame_hash(&f.pins.view.manifest, &raw)?;
    assert!(
        f.pins.view().verify_anchor(&a).is_err(),
        "checkpoint borrowed a dependent selection from after its durable pin prefix"
    );
    Ok(())
}

#[test]
fn t88_missing_retention_marker_cannot_report_durable_readiness() -> Result<()> {
    let f = fixture(true)?;
    std::fs::remove_file(f.pins.objects().join("retention.required.json"))?;
    let result = ReadOnlyPins::open(f.source.root(), f.source.network(), f.source.anchor_hash())
        .and_then(|v| v.unwrap().verify(&f.source, &[f.request], 4));
    assert!(
        result.is_err(),
        "durable retention deletion guard was missing"
    );
    Ok(())
}

#[test]
fn t81_corrupt_record_has_a_typed_integrity_class_and_preserves_bytes() -> Result<()> {
    let f = fixture(true)?;
    let path = f
        .source
        .root()
        .join(DIR)
        .join("records")
        .join(&f.pins.view.files[0].0);
    let bad = b"corrupt pin record";
    std::fs::write(&path, bad)?;
    let e = ReadOnlyPins::open(f.source.root(), f.source.network(), f.source.anchor_hash())
        .err()
        .expect("corrupt record accepted");
    assert!(
        matches!(
            e.downcast_ref::<crate::error::Error>(),
            Some(crate::error::Error::Integrity { .. })
        ),
        "untyped corruption: {e:#}"
    );
    assert_eq!(std::fs::read(path)?, bad);
    Ok(())
}

#[test]
fn t88_disposable_cache_retirement_refuses_the_authoritative_pin_store() -> Result<()> {
    let f = fixture(true)?;
    let object_root = f.pins.objects();
    let digest = f.artifacts[2].digest;
    let retained = f.pins.view().artifact_path(digest);
    let before = std::fs::read(&retained)?;
    drop(f.pins);
    let objects = crate::objects::Objects::open(&object_root)?;
    assert!(objects.retire(&hex_string(&digest), |_| Ok(())).is_err());
    assert_eq!(std::fs::read(retained)?, before);
    Ok(())
}

#[test]
fn interval_union_matches_a_bounded_membership_oracle_and_u64_edges() {
    let mut union = IntervalUnion::default();
    let mut membership = [false; 257];
    for n in 0..1024u64 {
        let lo = (n * 47 + 3) % 257;
        let hi = (lo + n % 17).min(256);
        union.insert(lo, hi);
        for h in lo..=hi {
            membership[h as usize] = true;
        }
        for (h, expected) in membership.iter().enumerate() {
            assert_eq!(union.contains(h as u64), *expected);
        }
        let pairs: Vec<_> = union.0.iter().collect();
        assert!(pairs.windows(2).all(|w| *w[0].1 + 1 < *w[1].0));
    }
    let mut edge = IntervalUnion::default();
    edge.insert(u64::MAX - 3, u64::MAX - 2);
    edge.insert(u64::MAX, u64::MAX);
    edge.insert(u64::MAX - 1, u64::MAX - 1);
    assert_eq!(edge.0, [(u64::MAX - 3, u64::MAX)].into());
    edge.insert(2, 1); // Empty input never widens retention.
    assert!(!edge.contains(1));
}

#[test]
fn t88_missing_manifest_stops_as_availability_without_recreating_authority() -> Result<()> {
    let f = fixture(true)?;
    let path = f.source.root().join(DIR).join(MANIFEST);
    let backup = f._dir.path().join("preserved-manifest");
    std::fs::rename(&path, &backup)?;
    let records = f.pins.view.files.clone();
    drop(f.pins);
    let error = PinStore::open(&f.source)
        .err()
        .expect("missing manifest was recreated");
    assert!(
        matches!(
            error.downcast_ref::<crate::error::Error>(),
            Some(crate::error::Error::Availability { .. })
        ),
        "{error:#}"
    );
    assert!(!path.exists());
    std::fs::rename(backup, &path)?;
    let restored = PinStore::open(&f.source)?;
    assert_eq!(restored.view.files, records);
    restored.view().verify(&f.source, &[f.request], 4)?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn t88_immutable_artifacts_hash_once_per_version_and_restoration_reverifies() -> Result<()> {
    let mut f = fixture(true)?;
    let fresh =
        ReadOnlyPins::open(f.source.root(), f.source.network(), f.source.anchor_hash())?.unwrap();
    ARTIFACT_HASHES.with(|c| c.borrow_mut().clear());
    for _ in 0..3 {
        fresh.verify(&f.source, std::slice::from_ref(&f.request), 4)?;
    }
    let counts = ARTIFACT_HASHES.with(|c| c.borrow().clone());
    assert_eq!(counts.len(), 3);
    assert!(
        counts.values().all(|n| *n == 1),
        "same immutable artifacts repeatedly hashed: {counts:?}"
    );
    let a = &f.artifacts[1];
    let path = f.pins.view().artifact_path(a.digest);
    let saved = std::fs::read(&path)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    std::fs::write(&path, b"changed pinned binary")?;
    assert!(
        fresh
            .verify_releases(&f.request.gates, &f.request.components)
            .is_err(),
        "cached verification hid a changed file"
    );
    let bad = f
        .pins
        .refresh(&f.source)
        .expect_err("new version was not hashed");
    assert!(
        matches!(
            bad.downcast_ref::<crate::error::Error>(),
            Some(crate::error::Error::Integrity { .. })
        ),
        "{bad:#}"
    );
    assert_eq!(std::fs::read(&path)?, b"changed pinned binary");
    std::fs::write(&path, &saved)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500))?;
    f.pins.refresh(&f.source)?;
    f.pins.view().verify(&f.source, &[f.request], 4)?;
    assert!(
        ARTIFACT_HASHES.with(|c| c.borrow()[&a.digest] >= 3),
        "restored version inherited stale verification"
    );
    Ok(())
}

#[test]
fn fix1_writer_refresh_and_verify_do_not_reread_the_verified_prefix() -> Result<()> {
    let mut f = fixture(true)?;
    f.pins
        .view()
        .verify(&f.source, std::slice::from_ref(&f.request), 4)?;
    PIN_COST.with(|c| c.set((0, 0, 0)));
    f.pins.refresh(&f.source)?;
    f.pins
        .view()
        .verify(&f.source, std::slice::from_ref(&f.request), 4)?;
    f.pins
        .view()
        .verify(&f.source, std::slice::from_ref(&f.request), 4)?;
    let parent = f.source.get(4)?.block.hash;
    f.source.append(&empty_fixture(
        5,
        boole_derivation::attributes::hex_bytes(&parent, "parent")?
            .try_into()
            .unwrap(),
    ))?;
    let b = binding(&f.source, 5)?;
    f.pins.prepare(&f.source, b.clone(), &[f.request])?;
    f.pins.select(5, &b.header.hash, b.root)?;
    let costs = PIN_COST.with(|c| c.get());
    assert_eq!(costs.0, 0, "refresh reread old records: {costs:?}");
    assert!(costs.1 <= 1, "verify reread authenticated rows: {costs:?}");
    assert_eq!(costs.2, 0, "publication copied the whole index: {costs:?}");
    Ok(())
}
#[test]
fn fix1_refresh_adopts_only_the_durable_new_hash_chained_suffix() -> Result<()> {
    let mut f = fixture(true)?;
    let frame = Frame {
        version: 1,
        sequence: f.pins.view.files.len() as u64,
        previous: f.pins.view.last,
        event: Event::Bootstrapped,
    };
    let raw = serde_json::to_vec(&frame)?;
    let digest = frame_hash(&f.pins.view.manifest, &raw)?;
    let path = f.source.root().join(DIR).join("records").join(format!(
        "{:020}-{}.json",
        frame.sequence,
        hex::encode(digest)
    ));
    publish_new(&path, &raw)?;
    PIN_COST.with(|c| c.set((0, 0, 0)));
    f.pins.refresh(&f.source)?;
    assert_eq!(f.pins.view().record_id(), digest);
    assert_eq!(PIN_COST.with(|c| c.get().0), 1, "old prefix was read again");
    Ok(())
}

#[test]
#[cfg(unix)]
fn fix1_large_private_artifact_staging_recovers_without_manual_repair() -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let f = fixture(false)?;
    let objects = f.pins.objects();
    let stage = objects.join(format!(".devnet-new-{}", "a".repeat(32)));
    std::fs::DirBuilder::new().mode(0o700).create(&stage)?;
    let file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(stage.join("value"))?;
    let length = crate::input::ROW_MAX as u64 + 1;
    file.set_len(length)?;
    file.sync_all()?;
    drop(file);
    drop(f.pins);
    let p = PinStore::open(&f.source)?;
    assert!(!stage.exists(), "large orphan staging blocked startup");
    let saved = p
        .objects()
        .join("corrupt")
        .join(format!("publication-{}", "a".repeat(32)))
        .join("value");
    assert_eq!(std::fs::metadata(saved)?.len(), length);
    Ok(())
}

#[test]
#[cfg(unix)]
fn fix1_parent_directory_sync_failure_precedes_pin_authority() -> Result<()> {
    for suffix in ["", "records", "objects"] {
        let f = fixture(false)?;
        drop(f.pins);
        let root = f.source.root().join(DIR);
        std::fs::rename(&root, f._dir.path().join("preserved-pins"))?;
        let failed = if suffix.is_empty() {
            root.clone()
        } else {
            root.join(suffix)
        };
        crate::durability::fail_parent_sync(failed);
        let result = PinStore::open(&f.source);
        crate::durability::clear_parent_sync_failure();
        assert!(
            result.is_err(),
            "{suffix}: pin authority published before parent fsync"
        );
        assert!(
            !root.join(MANIFEST).exists(),
            "manifest escaped a directory durability failure"
        );
        drop(result);
        PinStore::open(&f.source)?;
    }
    Ok(())
}

#[path = "cost_tests.rs"]
mod cost;

#[test]
#[cfg(unix)]
fn fix1_large_linked_artifact_publication_completes_recovery() -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    let f = fixture(false)?;
    let root = f.pins.objects();
    let stage = root.join(format!(".devnet-new-{}", "c".repeat(32)));
    std::fs::DirBuilder::new().mode(0o700).create(&stage)?;
    let value = stage.join("value");
    let file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&value)?;
    let length = crate::input::ROW_MAX as u64 + 1;
    file.set_len(length)?;
    file.sync_all()?;
    drop(file);
    let published = root.join(hex_string(&[0; 32]));
    std::fs::hard_link(&value, &published)?;
    drop(f.pins);
    let pins = PinStore::open(&f.source)?;
    assert!(!stage.exists());
    let m = std::fs::metadata(published)?;
    assert_eq!(m.len(), length);
    assert_eq!(m.nlink(), 1);
    assert!(
        pins.view()
            .index
            .artifacts
            .values()
            .all(|a| a.digest != [0; 32]),
        "orphan bytes became pin authority"
    );
    Ok(())
}

#[test]
fn fix1_incremental_cursor_cannot_refresh_from_a_foreign_source() -> Result<()> {
    let mut f = fixture(true)?;
    let mut network = f.source.network().clone();
    network.l2_genesis_hash = hex_string(&[44; 32]);
    let other = InputJournal::open(
        &f._dir.path().join("foreign"),
        network,
        || f.source.row(0),
        false,
    )?;
    let before = f.pins.view().record_id();
    let e = f
        .pins
        .refresh(&other)
        .expect_err("warm cursor accepted a foreign source identity");
    assert!(
        matches!(
            e.downcast_ref::<crate::error::Error>(),
            Some(crate::error::Error::Startup { number: 20, .. })
        ),
        "{e:#}"
    );
    assert_eq!(f.pins.view().record_id(), before);
    Ok(())
}
