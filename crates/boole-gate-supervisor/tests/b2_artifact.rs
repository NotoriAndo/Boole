#[path = "support/controller_pins.rs"]
mod pin_fixture;
use boole_gate_supervisor::{
    codec::{digest, Scratch},
    real_artifact::local::ResolverV1,
    real_deadline::Deadline,
};
use serde_json::json;
use std::{collections::BTreeMap, fs};
#[test]
fn b2_art_local_linux_manifest_is_streamed_without_promoting_a_draft_release() -> anyhow::Result<()>
{
    let root = Scratch::new(&std::env::temp_dir())?;
    fs::create_dir(root.0.join("objects"))?;
    fs::create_dir(root.0.join("chunks"))?;
    let data = b"local public input";
    let id = digest(data);
    let hex = hex::encode(id);
    fs::write(root.0.join("chunks").join(&hex), data)?;
    let m = json!({"assembled":{"bytes":data.len(),"sha256":hex},"chunks":[{"bytes":data.len(),"index":0,"object":format!("chunks/{hex}"),"offset":0,"sha256":hex}],"format":"BOOLE-M3/LOCAL-ARTIFACT/v1","profile":"Linux-aarch64","role":"reference-export","version":1});
    let mut raw = serde_json::to_vec(&m)?;
    raw.push(b'\n');
    let pin = digest(&raw);
    fs::write(root.0.join("objects").join(hex::encode(pin)), raw)?;
    let r = ResolverV1 {
        root: root.0.clone(),
        manifests: BTreeMap::from([(id, pin)]),
        profile: "Linux-aarch64".into(),
    };
    let out = Scratch::new(&root.0)?;
    let installed = r.resolve(id, &out.0, Deadline::after(30000));
    assert!(
        installed.is_ok(),
        "valid local Linux transport refused: {:?}",
        installed.err()
    );
    assert_eq!(fs::read(out.0.join(hex))?, data);
    Ok(())
}

fn fixture(data: &[u8]) -> anyhow::Result<(Scratch, ResolverV1, serde_json::Value, [u8; 32])> {
    let root = Scratch::new(&std::env::temp_dir())?;
    for p in ["objects", "chunks"] {
        fs::create_dir(root.0.join(p))?;
    }
    let id = digest(data);
    let h = hex::encode(id);
    fs::write(root.0.join("chunks").join(&h), data)?;
    let chunks = if data.is_empty() {
        json!([])
    } else {
        json!([{"bytes":data.len(),"index":0,"object":format!("chunks/{h}"),"offset":0,"sha256":h}])
    };
    let m = json!({"assembled":{"bytes":data.len(),"sha256":h},"chunks":chunks,"format":"BOOLE-M3/LOCAL-ARTIFACT/v1","profile":"Linux-aarch64","role":"reference-export","version":1});
    let mut r = ResolverV1 {
        root: root.0.clone(),
        manifests: BTreeMap::new(),
        profile: "Linux-aarch64".into(),
    };
    install_manifest(&mut r, id, &m)?;
    Ok((root, r, m, id))
}
fn install_manifest(r: &mut ResolverV1, id: [u8; 32], m: &serde_json::Value) -> anyhow::Result<()> {
    let mut raw = serde_json::to_vec(m)?;
    raw.push(b'\n');
    install_raw(r, id, &raw)
}
fn install_raw(r: &mut ResolverV1, id: [u8; 32], raw: &[u8]) -> anyhow::Result<()> {
    let pin = digest(raw);
    fs::write(r.root.join("objects").join(hex::encode(pin)), raw)?;
    r.manifests.insert(id, pin);
    Ok(())
}
#[test]
fn b2_art_held_readers_keep_independent_cursors_and_refuse_path_replacement() -> anyhow::Result<()>
{
    use std::io::Read;
    let data = b"abcdef";
    let (root, r, _, id) = fixture(data)?;
    let out = Scratch::new(&root.0)?;
    let a = r.resolve(id, &out.0, Deadline::after(30000))?;
    assert_eq!(a.binding().sha256, id);
    assert_eq!(a.binding().bytes, 6);
    assert_eq!(a.max_read(), 6);
    let mut one = a.reader(Deadline::after(30000))?;
    let mut two = a.reader(Deadline::after(30000))?;
    let mut first = [0; 2];
    one.read_exact(&mut first)?;
    assert_eq!(&first, b"ab");
    let mut all = vec![];
    two.read_to_end(&mut all)?;
    assert_eq!(all, data);
    let mut rest = vec![];
    one.read_to_end(&mut rest)?;
    assert_eq!(rest, b"cdef");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let m = fs::metadata(a.path())?;
        assert_eq!(m.nlink(), 1);
        assert_eq!(m.permissions().mode() & 0o222, 0);
    }
    assert_eq!(fs::read(root.0.join("chunks").join(hex::encode(id)))?, data);
    let replacement = out.0.join("replacement");
    fs::write(&replacement, data)?;
    fs::rename(replacement, a.path())?;
    assert!(a.verify(Deadline::after(30000)).is_err());
    assert!(one.read(&mut first).is_err());
    Ok(())
}
#[test]
fn b2_art_all_local_manifest_and_chunk_refusals_preserve_existing_objects() -> anyhow::Result<()> {
    let (root, mut r, m, id) = fixture(b"fixture")?;
    for (key, value) in [
        ("version", json!(2)),
        ("profile", json!("Darwin-arm64")),
        ("role", json!("shell")),
        ("format", json!("EENV15-chunk-manifest-draft-v1")),
        ("unknown", json!(1)),
        ("assembled", json!({"bytes":8,"sha256":hex::encode(id)})),
        (
            "chunks",
            json!([{"bytes":7,"index":1,"object":format!("chunks/{}",hex::encode(id)),"offset":0,"sha256":hex::encode(id)}]),
        ),
    ] {
        let mut bad = m.clone();
        bad[key] = value;
        install_manifest(&mut r, id, &bad)?;
        let out = Scratch::new(&root.0)?;
        assert!(
            r.resolve(id, &out.0, Deadline::after(30000)).is_err(),
            "accepted {key}"
        );
        assert!(!out.0.join(hex::encode(id)).exists());
    }
    let mut raw = serde_json::to_vec(&m)?;
    raw.push(b'\n');
    let noncanonical = [raw.as_slice(), b" "].concat();
    install_raw(&mut r, id, &noncanonical)?;
    let out = Scratch::new(&root.0)?;
    assert!(r.resolve(id, &out.0, Deadline::after(30000)).is_err());
    let duplicate =
        String::from_utf8(raw.clone())?.replacen("\"version\":1", "\"version\":1,\"version\":1", 1);
    install_raw(&mut r, id, duplicate.as_bytes())?;
    assert!(r.resolve(id, &out.0, Deadline::after(30000)).is_err());
    install_manifest(&mut r, id, &m)?;
    let chunk = root.0.join("chunks").join(hex::encode(id));
    fs::write(&chunk, b"corrupt")?;
    assert!(r.resolve(id, &out.0, Deadline::after(30000)).is_err());
    assert!(!out.0.join(hex::encode(id)).exists());
    fs::write(&chunk, b"fixture")?;
    #[cfg(unix)]
    {
        let saved = root.0.join("saved");
        fs::rename(&chunk, &saved)?;
        std::os::unix::fs::symlink(&saved, &chunk)?;
        assert!(r.resolve(id, &out.0, Deadline::after(30000)).is_err());
        fs::remove_file(&chunk)?;
        fs::rename(&saved, &chunk)?;
    }
    fs::write(out.0.join(hex::encode(id)), b"retain conflicting object")?;
    assert!(r.resolve(id, &out.0, Deadline::after(30000)).is_err());
    assert_eq!(
        fs::read(out.0.join(hex::encode(id)))?,
        b"retain conflicting object"
    );
    fs::remove_file(&chunk)?;
    let fresh = Scratch::new(&root.0)?;
    assert!(r.resolve(id, &fresh.0, Deadline::after(30000)).is_err());
    fs::write(&chunk, b"fixture")?;
    let a = r.resolve(id, &fresh.0, Deadline::after(30000))?;
    a.verify(Deadline::after(30000))?;
    assert!(
        r.resolve(id, &fresh.0, Deadline::after(30000)).is_ok(),
        "verified publication must be restart-idempotent"
    );
    r.manifests.clear();
    assert!(r.resolve(id, &fresh.0, Deadline::after(30000)).is_err());
    Ok(())
}
#[test]
fn b2_art_streams_over_legacy_read_limit_and_empty_objects() -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};
    let (root, mut r, mut m, _) = fixture(b"x")?;
    let zero = vec![0; 65536];
    let mut ch = Sha256::new();
    for _ in 0..1024 {
        ch.update(&zero);
    }
    let chunk: [u8; 32] = ch.finalize().into();
    let f = fs::File::create(root.0.join("chunks").join(hex::encode(chunk)))?;
    f.set_len(67108864)?;
    drop(f);
    let tail = digest(&[0]);
    fs::write(root.0.join("chunks").join(hex::encode(tail)), [0])?;
    let mut full = Sha256::new();
    for _ in 0..4096 {
        full.update(&zero);
    }
    full.update([0]);
    let id: [u8; 32] = full.finalize().into();
    m["assembled"] = json!({"bytes":268435457u64,"sha256":hex::encode(id)});
    m["chunks"]=serde_json::Value::Array((0..5u64).map(|i|{let h=hex::encode(if i<4{chunk}else{tail});json!({"bytes":if i<4{67108864}else{1},"index":i,"object":format!("chunks/{h}"),"offset":i*67108864,"sha256":h})}).collect());
    install_manifest(&mut r, id, &m)?;
    let out = Scratch::new(&root.0)?;
    let a = r.resolve(id, &out.0, Deadline::after(120000))?;
    assert_eq!(a.binding().bytes, 268435457);
    assert_eq!(a.max_read(), 65536);
    a.verify(Deadline::after(120000))?;
    let (empty, r, _, id) = fixture(b"")?;
    let out = Scratch::new(&empty.0)?;
    let a = r.resolve(id, &out.0, Deadline::after(30000))?;
    assert_eq!(a.binding().bytes, 0);
    a.verify(Deadline::after(30000))?;
    Ok(())
}

#[test]
fn b2_art_controller_accepts_only_independently_bound_held_inputs() -> anyhow::Result<()> {
    use boole_gate_supervisor::{
        real_artifact::local::{Profile, Role},
        real_controller::{Input, Invocation, Limits, Phase, Request, Tool},
        real_reply::Context,
    };
    let (root, r, _, id) = fixture(b"verified input")?;
    let out = Scratch::new(&root.0)?;
    for (expected, role, profile) in [
        ([0; 32], Role::ReferenceExport, Profile::LinuxAarch64),
        (id, Role::Runtime, Profile::LinuxAarch64),
        (id, Role::ReferenceExport, Profile::DarwinArm64),
    ] {
        let held = r.resolve(id, &out.0, Deadline::after(30000))?;
        assert!(Input::new(held, expected, role, profile, Deadline::after(30000)).is_err());
    }
    let descriptor = include_bytes!("../../../fixtures/m3-real-gate/descriptor-v1.json");
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/m3-real-gate/reply-cases.json"
    ))?;
    let context: Context = serde_json::from_value(value["context"].clone())?;
    let input = || {
        Input::new(
            r.resolve(id, &out.0, Deadline::after(30000))?,
            id,
            Role::ReferenceExport,
            Profile::LinuxAarch64,
            Deadline::after(30000),
        )
    };
    let invocation = |profile| Invocation {
        descriptor,
        context: &context,
        epoch: 0,
        decoder: [7; 32],
        transport: [8; 32],
        profile,
    };
    let limits = Limits {
        memory_mib: 512,
        wall_ms: 1000,
        output_bytes: 65536,
    };
    let request = Request::new_with_pins(
        Phase::Front,
        Tool::Front,
        limits,
        invocation(Profile::LinuxAarch64),
        vec![input()?],
        Deadline::after(30000),
        &pin_fixture::Pins {
            decoder: Some([7; 32]),
            transport: Some([8; 32]),
        },
    )?;
    assert_eq!(request.inputs()[0].artifact().binding().sha256, id);
    assert!(Request::new_with_pins(
        Phase::Front,
        Tool::Front,
        limits,
        invocation(Profile::DarwinArm64),
        vec![input()?],
        Deadline::after(30000),
        &pin_fixture::Pins {
            decoder: Some([7; 32]),
            transport: Some([8; 32])
        }
    )
    .is_err());
    assert!(Request::new_with_pins(
        Phase::Kernel,
        Tool::Decoder,
        limits,
        invocation(Profile::LinuxAarch64),
        vec![],
        Deadline::after(30000),
        &pin_fixture::Pins {
            decoder: Some([7; 32]),
            transport: Some([8; 32])
        }
    )
    .is_err());
    assert!(Request::new_with_pins(
        Phase::Front,
        Tool::Front,
        Limits {
            output_bytes: 65537,
            ..limits
        },
        invocation(Profile::LinuxAarch64),
        vec![],
        Deadline::after(30000),
        &pin_fixture::Pins {
            decoder: Some([7; 32]),
            transport: Some([8; 32])
        }
    )
    .is_err());
    Ok(())
}
