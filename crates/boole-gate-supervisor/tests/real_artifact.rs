use boole_gate_supervisor::{
    codec::{digest, Scratch},
    real_artifact::Resolver,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
fn hex(h: [u8; 32]) -> String {
    hex::encode(h)
}
fn manifest(bytes: &[u8]) -> Value {
    let sha = hex(digest(bytes));
    json!({"format":"EENV15-chunk-manifest-draft-v1","version":1,"status":"draft","seal":"unsealed","activation":false,"profile":"Darwin-arm64","role":"reference-export","max_chunk_bytes":67108864,"assembled":{"sha256":sha,"bytes":bytes.len()},"chunks":[{"index":0,"offset":0,"bytes":bytes.len(),"sha256":sha,"object":format!("chunks/{sha}")}],"storage":"manifest-only","storage_reason":"Development fixture only: 제안","authentication":{"value":null,"reason":"D7: reviewed D/N release authentication and certificate issuance pending"}})
}
fn resolver(root: &std::path::Path, m: &Value, id: [u8; 32]) -> anyhow::Result<Resolver> {
    let mut raw = serde_json::to_vec(m)?;
    raw.push(b'\n');
    let pin = digest(&raw);
    std::fs::write(root.join("objects").join(hex(pin)), raw)?;
    Ok(Resolver {
        root: root.into(),
        manifests: BTreeMap::from([(id, pin)]),
    })
}
#[test]
fn d4_artifact_authentication_schema_chunks_and_publication_are_fail_closed() -> anyhow::Result<()>
{
    let root = Scratch::new(&std::env::temp_dir())?;
    std::fs::create_dir(root.0.join("objects"))?;
    std::fs::create_dir(root.0.join("chunks"))?;
    let raw = b"public artifact";
    let id = digest(raw);
    let chunk = root.0.join("chunks").join(hex(id));
    std::fs::write(&chunk, raw)?;
    let good = manifest(raw);
    let mut accepted = vec![];
    for (name, mutation) in [
        (
            "assembled-hash",
            json!({"assembled":{"sha256":hex([9;32]),"bytes":raw.len()}}),
        ),
        (
            "assembled-size",
            json!({"assembled":{"sha256":hex(id),"bytes":raw.len()+1}}),
        ),
        ("wrong-profile", json!({"profile":"Linux-aarch64"})),
        ("activation", json!({"activation":true})),
        ("unknown-field", json!({"unexpected":1})),
        ("chunk-cap", json!({"max_chunk_bytes":67108865})),
        (
            "chunk-index",
            json!({"chunks":[{"index":1,"offset":0,"bytes":raw.len(),"sha256":hex(id),"object":format!("chunks/{}",hex(id))}]}),
        ),
        (
            "chunk-offset",
            json!({"chunks":[{"index":0,"offset":1,"bytes":raw.len(),"sha256":hex(id),"object":format!("chunks/{}",hex(id))}]}),
        ),
        (
            "chunk-object",
            json!({"chunks":[{"index":0,"offset":0,"bytes":raw.len(),"sha256":hex([8;32]),"object":format!("chunks/{}",hex(id))}]}),
        ),
        (
            "nonfinal-short",
            json!({"chunks":[{"index":0,"offset":0,"bytes":raw.len(),"sha256":hex(id),"object":format!("chunks/{}",hex(id))},{"index":1,"offset":raw.len(),"bytes":raw.len(),"sha256":hex(id),"object":format!("chunks/{}",hex(id))}]}),
        ),
    ] {
        let mut bad = good.clone();
        for (key, value) in mutation.as_object().unwrap() {
            bad[key] = value.clone();
        }
        let out = Scratch::new(&root.0)?;
        if resolver(&root.0, &bad, id)?.resolve(id, &out.0).is_ok() {
            accepted.push(name);
        }
    }
    let out = Scratch::new(&root.0)?;
    let r = resolver(&root.0, &good, id)?;
    let object = root.0.join("objects").join(hex(r.manifests[&id]));
    let original = std::fs::read(&object)?;
    std::fs::write(&object, [original.as_slice(), b"\n"].concat())?;
    if r.resolve(id, &out.0).is_ok() {
        accepted.push("outer-manifest-hash");
    }
    std::fs::write(&object, &original)?;
    std::fs::write(&chunk, b"corrupt bytes!!")?;
    if r.resolve(id, &out.0).is_ok() {
        accepted.push("chunk-content-hash");
    }
    std::fs::write(&chunk, raw)?;
    #[cfg(unix)]
    {
        let saved = root.0.join("saved");
        std::fs::rename(&chunk, &saved)?;
        std::os::unix::fs::symlink(&saved, &chunk)?;
        if r.resolve(id, &out.0).is_ok() {
            accepted.push("chunk-symlink");
        }
        std::fs::remove_file(&chunk)?;
        std::fs::rename(&saved, &chunk)?;
    }
    let missing = Resolver {
        root: root.0.clone(),
        manifests: BTreeMap::new(),
    };
    assert!(missing.resolve(id, &out.0).is_err());
    let target = out.0.join(hex(id));
    std::fs::write(&target, b"preserve")?;
    if r.resolve(id, &out.0).is_ok() || std::fs::read(&target)? != b"preserve" {
        accepted.push("overwrite");
    }
    assert!(accepted.is_empty(), "D4 rules accepted: {accepted:?}");
    let normal = Scratch::new(&root.0)?;
    let a = r.resolve(id, &normal.0)?;
    assert_eq!(std::fs::read(&a.path)?, raw);
    assert!(a.max_read <= 65536);
    Ok(())
}
#[test]
fn d4_streams_above_the_legacy_read_cap_and_accepts_the_empty_file() -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let root = Scratch::new(&std::env::temp_dir())?;
    std::fs::create_dir(root.0.join("objects"))?;
    std::fs::create_dir(root.0.join("chunks"))?;
    let block = [0u8; 65536];
    let mut ch = Sha256::new();
    let source = root.0.join("source");
    let mut file = std::fs::File::create(&source)?;
    for _ in 0..1024 {
        file.write_all(&block)?;
        ch.update(block);
    }
    drop(file);
    let chunk: [u8; 32] = ch.finalize().into();
    std::fs::rename(source, root.0.join("chunks").join(hex(chunk)))?;
    let tail = digest(&[0]);
    std::fs::write(root.0.join("chunks").join(hex(tail)), [0])?;
    let mut full = Sha256::new();
    for _ in 0..4096 {
        full.update(block);
    }
    full.update([0]);
    let id: [u8; 32] = full.finalize().into();
    let mut m = manifest(b"x");
    m["assembled"] = json!({"sha256":hex(id),"bytes":268435457u64});
    m["chunks"]=Value::Array((0..5u64).map(|i|json!({"index":i,"offset":i*67108864,"bytes":if i<4 {67108864} else {1},"sha256":hex(if i<4 {chunk}else{tail}),"object":format!("chunks/{}",hex(if i<4 {chunk}else{tail}))})).collect());
    let out = Scratch::new(&root.0)?;
    let a = resolver(&root.0, &m, id)?.resolve(id, &out.0)?;
    assert_eq!(a.bytes, 268435457);
    assert_eq!(a.max_read, 65536);
    a.verify()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&a.path)?.permissions().mode() & 0o222, 0);
    }
    let id = digest(b"");
    let mut empty = manifest(b"");
    empty["chunks"] = json!([]);
    let a = resolver(&root.0, &empty, id)?.resolve(id, &out.0)?;
    assert_eq!(a.bytes, 0);
    assert_eq!(a.max_read, 0);
    a.verify()?;
    Ok(())
}
