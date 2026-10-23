#![cfg(feature = "qualification-faults")]
use anyhow::{ensure, Result};
use boole_devnet_archive::{durability::PrivateTempDir, fault, journal, objects::Objects};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const KEY: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BODY: &[u8] = b"public bytes must survive a pre-link crash";
#[test]
#[ignore = "fixture subprocess only"]
fn publication_child() -> Result<()> {
    let root = std::env::var_os("BOOLE_PUBLICATION_CHILD").unwrap();
    let root = Path::new(&root);
    let _objects = Objects::open(root)?;
    journal::publish_new_at(&root.join(&KEY[2..]), BODY, Some((1, "publication")))
}
fn crash(point: &str) -> Result<()> {
    let d = PrivateTempDir::new("m4b-f1")?;
    let marker = d.path().join("marker.json");
    let store = d.path().join("store");
    drop(Objects::open(&store)?);
    let mut child = Command::new(std::env::current_exe()?)
        .args(["--exact", "publication_child", "--ignored", "--nocapture"])
        .env("BOOLE_PUBLICATION_CHILD", &store)
        .env("BOOLE_M4_KILL_POINT", point)
        .env("BOOLE_M4_KILL_HEIGHT", "1")
        .env("BOOLE_M4_KILL_ROLE", "publication")
        .env("BOOLE_M4_KILL_MARKER", &marker)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let result = (|| -> Result<()> {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if fault::stopped_marker(&marker, child.id())?.is_some() {
                break;
            }
            ensure!(
                child.try_wait()?.is_none(),
                "{point} hook missed: writer exited before the kill boundary"
            );
            ensure!(Instant::now() < end, "{point} marker timeout");
            std::thread::sleep(Duration::from_millis(10));
        }
        child.kill()?;
        ensure!(!child.wait()?.success(), "SIGKILL required");
        let objects = Objects::open(&store)?;
        if point == "B9a" {
            ensure!(
                objects.read(KEY, |_| Ok(()))?.is_none(),
                "unpublished value became authority"
            );
            let entries =
                std::fs::read_dir(store.join("corrupt"))?.collect::<std::io::Result<Vec<_>>>()?;
            ensure!(
                entries.len() == 1,
                "unpublished bytes not preserved exactly once"
            );
            ensure!(
                std::fs::read(entries[0].path().join("value"))? == BODY,
                "quarantine lost bytes"
            );
            objects.put(KEY, BODY, |b| {
                ensure!(b == BODY, "bytes");
                Ok(())
            })?;
        }
        ensure!(
            objects
                .read(KEY, |b| {
                    ensure!(b == BODY, "bytes");
                    Ok(())
                })?
                .as_deref()
                == Some(BODY),
            "restart did not recover object"
        );
        ensure!(
            !std::fs::read_dir(&store)?.any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".devnet-new-")),
            "staging survived restart"
        );
        drop(objects);
        drop(Objects::open(&store)?);
        Ok(())
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}
#[test]
fn f1_b9a_pre_link_preserves_and_republishes() -> Result<()> {
    crash("B9a")
}
#[test]
fn f1_b9b_post_unlink_removes_the_empty_container() -> Result<()> {
    crash("B9b")
}
