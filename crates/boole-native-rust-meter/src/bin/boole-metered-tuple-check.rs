//! Explicit local-only, non-issuable checker. No services or compiler process.
use boole_native_rust_meter::tuple_verifier::{
    verify, InputError, Verdict, MAX_ANSWER_BYTES, MAX_TASK_BYTES,
};
use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, &'static str> {
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| "read_failed")?
    };
    #[cfg(not(unix))]
    let file = std::fs::File::open(path).map_err(|_| "read_failed")?;
    let metadata = file.metadata().map_err(|_| "read_failed")?;
    if !metadata.is_file() {
        return Err("read_failed");
    }
    if metadata.len() > limit as u64 {
        return Err("too_large");
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "read_failed")?;
    if bytes.len() > limit {
        return Err("too_large");
    }
    Ok(bytes)
}

fn diagnostic(outcome: &str, reason: &str) -> (u8, Vec<u8>) {
    (
        2,
        serde_json::to_vec(&serde_json::json!({
            "schema": "boole.metered-tuple-local-status.v1",
            "outcome": outcome,
            "reason": reason,
            "nonIssuable": true,
            "activationAllowed": false,
        }))
        .expect("constant status serialization"),
    )
}

fn run() -> (u8, Vec<u8>) {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.len() != 2 {
        return diagnostic(
            "input_error",
            "usage: boole-metered-tuple-check TASK_JSON ANSWER_BODY",
        );
    }
    let mut inputs = Vec::new();
    for (name, path, limit) in [
        ("task", &arguments[0], MAX_TASK_BYTES),
        ("answer", &arguments[1], MAX_ANSWER_BYTES),
    ] {
        match read_bounded(Path::new(path), limit) {
            Ok(bytes) => inputs.push(bytes),
            Err(reason) => {
                return diagnostic(
                    if reason == "too_large" {
                        "input_error"
                    } else {
                        "retryable_unavailable"
                    },
                    &format!("{name}_{reason}"),
                );
            }
        }
    }
    match verify(&inputs[0], &inputs[1]) {
        Ok(result) => (
            u8::from(result.verdict() != Verdict::Accepted),
            result.canonical_bytes(),
        ),
        Err(error) => diagnostic(
            "input_error",
            match error {
                InputError::TaskTooLarge => "task_too_large",
                InputError::InvalidTaskSpecification => "invalid_task_specification",
                InputError::AnswerTooLarge => "answer_too_large",
            },
        ),
    }
}

fn main() -> ExitCode {
    let (code, mut bytes) = run();
    bytes.push(b'\n');
    if std::io::stdout().lock().write_all(&bytes).is_err() {
        return ExitCode::from(2);
    }
    ExitCode::from(code)
}
