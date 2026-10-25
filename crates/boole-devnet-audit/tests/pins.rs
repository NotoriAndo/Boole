//! T81: readiness is independent of mathematical replay and never repairs pins.
use anyhow::Result;
use boole_derivation::{
    attributes::hex_string,
    m2::{Config, State},
    m3::{ForkTable, M3Config},
    ParentHeader,
};
use boole_devnet_archive::{
    durability::PrivateTempDir,
    input::InputRow,
    journal::{InputJournal, NetworkIdentity, ReadOnlyJournal},
    pins::{ArtifactRole, HeaderBinding, PinStore},
};
use boole_devnet_audit::{audit, ExpectedHeads, JournalInputs, Node};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
struct View {
    header: ParentHeader,
    mutate: Option<PathBuf>,
}
impl Node for View {
    fn head(&mut self) -> Result<ParentHeader> {
        if let Some(p) = self.mutate.take() {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .append(true)
                .open(p)?
                .write_all(b" ")?;
        }
        Ok(self.header.clone())
    }
    fn header(&mut self, _: u64) -> Result<ParentHeader> {
        Ok(self.header.clone())
    }
    fn transactions(&mut self, _: u64) -> Result<Vec<String>> {
        unreachable!()
    }
    fn receipt_statuses(&mut self, _: u64) -> Result<Vec<bool>> {
        unreachable!()
    }
    fn depositor_balance(&mut self, _: u64) -> Result<u128> {
        unreachable!()
    }
}
struct Fixture {
    _dir: PrivateTempDir,
    source: InputJournal,
    pins: PinStore,
    c: Config,
    f: ForkTable,
    header: ParentHeader,
    artifact: PathBuf,
}
fn setup() -> Result<Fixture> {
    let dir = PrivateTempDir::new("m4c2-audit-pins")?;
    let v: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m4/input-auth-v0.json"
    ))?;
    let row: InputRow = serde_json::from_value(v["rows"][0].clone())?;
    let mut c = Config::devnet();
    c.m3 = Some(M3Config::devnet(c.slot_table.len()));
    let f = ForkTable::genesis(&c)?;
    let header = ParentHeader {
        number: 0,
        hash: hex_string(&[1; 32]),
        parent_hash: None,
        timestamp: 0,
        gas_limit: 30_000_000,
        miner: hex_string(&[0; 20]),
        extra_data: "0x".into(),
        l1_info: None,
        m2_root_deposit: None,
    };
    let network = NetworkIdentity::new(&c, Some(&f), header.hash.clone(), header.gas_limit)?;
    let source = InputJournal::open(&dir.path().join("node"), network, || Ok(row), false)?;
    let mut pins = PinStore::open(&source)?;
    let body = b"retained public stub component";
    let digest = Sha256::digest(body).into();
    pins.install_bytes(
        ArtifactRole::Environment { component: digest },
        digest,
        body,
        false,
    )?;
    let artifact = pins.view().artifact_path(digest);
    let root = State::new(&c, header.gas_limit)?.root;
    pins.prepare(
        &source,
        HeaderBinding {
            header: header.clone(),
            transactions: vec![],
            root,
        },
        &[],
    )?;
    pins.select(0, &header.hash, root)?;
    Ok(Fixture {
        _dir: dir,
        source,
        pins,
        c,
        f,
        header,
        artifact,
    })
}
fn run(f: &Fixture, mutate: Option<&Path>) -> Result<serde_json::Value> {
    let mut source = JournalInputs(ReadOnlyJournal::open(f.source.root(), f.source.network())?);
    let mut view = View {
        header: f.header.clone(),
        mutate: mutate.map(Path::to_path_buf),
    };
    let expected = ExpectedHeads {
        l1_number: 0,
        l1_hash: f.source.anchor_hash().into(),
        l2: Some((0, f.header.hash.clone())),
    };
    Ok(audit(
        &mut source,
        &mut view,
        &f.c,
        Some(&f.f),
        None,
        &expected,
    ))
}
#[test]
fn t81_k4_ok_requires_rebuild_and_a_durable_read_only_pin_view() -> Result<()> {
    let f = setup()?;
    // Both owning writer locks remain held; audit takes no writer lock itself.
    let id = f.pins.view().record_id();
    let r = run(&f, None)?;
    assert!(r["first_mismatch"].is_null(), "{r}");
    assert_eq!(
        r["k4"]["status"], "ok",
        "durable readiness was never checked: {r}"
    );
    assert_eq!(r["k4"]["pin_retention"], "ok");
    assert_eq!(r["k4"]["verification_height"], "0");
    assert_eq!(r["k4"]["checked"], "0");
    assert_eq!(r["environment"]["status"], "pending");
    assert_eq!(f.pins.view().record_id(), id);
    f.pins.view().check()?;
    Ok(())
}
#[test]
fn t81_missing_pin_never_reports_ok_or_repairs_the_audited_source() -> Result<()> {
    let f = setup()?;
    let saved = f._dir.path().join("withheld");
    std::fs::rename(&f.artifact, &saved)?;
    let r = run(&f, None)?;
    assert!(r["first_mismatch"].is_null(), "{r}");
    assert_eq!(r["k4"]["status"], "pending");
    assert_eq!(r["k4"]["pin_retention"], "local-unavailable");
    assert!(!f.artifact.exists());
    assert!(saved.exists());
    assert!(!f.source.root().join("pins-v1/corrupt").exists());
    std::fs::rename(saved, &f.artifact)?;
    assert_eq!(run(&f, None)?["k4"]["status"], "ok");
    Ok(())
}
#[test]
fn t81_corrupt_durable_pin_is_k4_failure_and_keeps_its_bytes() -> Result<()> {
    let f = setup()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f.artifact, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::write(&f.artifact, b"corrupt pin")?;
    let r = run(&f, None)?;
    assert_eq!(
        r["first_mismatch"],
        serde_json::json!({"height":"0","class":"K4"})
    );
    assert_eq!(std::fs::read(&f.artifact)?, b"corrupt pin");
    Ok(())
}
#[test]
fn t81_concurrent_pin_version_change_is_pending_not_false_ok() -> Result<()> {
    let f = setup()?;
    let marker = f
        .source
        .root()
        .join("pins-v1/objects/retention.required.json");
    let r = run(&f, Some(&marker))?;
    assert!(r["first_mismatch"].is_null(), "{r}");
    assert_eq!(r["k4"]["status"], "pending");
    assert_eq!(r["k4"]["pin_retention"], "local-unavailable");
    Ok(())
}
