use std::process::Command;

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/native-metered-tuple-v1")
        .join(name)
}

#[test]
fn independent_processes_reproduce_exact_verdict_bytes_without_host_environment_inputs() {
    let handles = (0..4)
        .map(|index| {
            std::thread::spawn(move || {
                Command::new(env!("CARGO_BIN_EXE_boole-metered-tuple-check"))
                    .args([fixture("task.json"), fixture("answer.rs")])
                    .env_clear()
                    .env("BOOLE_TEST_HOST_MARKER", index.to_string())
                    .env("BOOLE_MAX_FUEL", "0")
                    .env("RUSTFLAGS", "--not-a-real-compiler-flag")
                    .output()
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let outputs = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    let corpus: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("corpus.json")).unwrap()).unwrap();
    let mut expected = serde_json::to_vec(&corpus["cases"][0]["expected"]).unwrap();
    expected.push(b'\n');
    for output in outputs {
        assert!(output.status.success(), "{:?}", output);
        assert_eq!(output.stdout, expected);
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn missing_task_is_unavailable_not_an_answer_rejection() {
    let output = Command::new(env!("CARGO_BIN_EXE_boole-metered-tuple-check"))
        .args(["/definitely-missing-boole-task.json", "/unused-answer.rs"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["outcome"], "retryable_unavailable");
    assert_eq!(result["reason"], "task_read_failed");
    assert!(result.get("verdict").is_none());
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "boole-tuple-cli-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
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

#[test]
fn cli_rejects_wrong_answers_and_refuses_oversized_or_invalid_input_without_a_verdict() {
    let scratch = Scratch::new();
    let answer = scratch.0.join("answer");
    for (bytes, code, field, expected) in [
        (b"7".to_vec(), 1, "reason", "answer_mismatch"),
        (vec![b' '; 8193], 2, "reason", "answer_too_large"),
    ] {
        std::fs::write(&answer, bytes).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_boole-metered-tuple-check"))
            .args([fixture("task.json"), answer.clone()])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(code));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value[field], expected);
        if code == 2 {
            assert!(value.get("verdict").is_none());
        }
    }
    let task = scratch.0.join("task");
    std::fs::write(&task, b"{}").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_boole-metered-tuple-check"))
        .args([task, fixture("answer.rs")])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["outcome"], "input_error");
    assert!(value.get("verdict").is_none());
}

#[cfg(unix)]
#[test]
fn symlink_and_fifo_inputs_are_unavailable_and_do_not_block() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new();
    let link = scratch.0.join("link");
    let fifo = scratch.0.join("fifo");
    symlink(fixture("task.json"), &link).unwrap();
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    for path in [link, fifo, scratch.0.clone()] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_boole-metered-tuple-check"))
            .args([path, fixture("answer.rs")])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("non-regular input blocked the local verifier");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["outcome"], "retryable_unavailable");
        assert!(value.get("verdict").is_none());
    }
}
