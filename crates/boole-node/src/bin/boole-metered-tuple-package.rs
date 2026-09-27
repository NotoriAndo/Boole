//! Explicit offline developer tool. Stop every other owner before opening a
//! CAS directory; the existing LocalPackageStore is single-owner, not a daemon.
use boole_core::{LocalPackageStore, LocalPackageStoreConfig, PackageRoot};
use boole_native_rust_meter::tuple_verifier::{Verdict, MAX_ANSWER_BYTES, MAX_TASK_BYTES};
use boole_node::metered_tuple_package::{
    build_package, import_package, restore_package, reverify_stored, ReverifyError,
    StoredVerification, MAX_METERED_PACKAGE_BYTES,
};
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;

type Response = (u8, Vec<u8>);
const USAGE: &str = concat!(
    "usage (offline, exclusive store ownership required): boole-metered-tuple-package ",
    "pack TASK ANSWER | import STORE EXPECTED_ROOT PACKAGE | ",
    "restore STORE EXPECTED_ROOT PACKAGE | verify STORE EXPECTED_ROOT"
);

fn json(code: u8, value: serde_json::Value) -> Response {
    let mut bytes = serde_json::to_vec(&value).expect("bounded JSON serialization");
    bytes.push(b'\n');
    (code, bytes)
}

fn status(outcome: &str, reason: &str) -> Response {
    json(
        2,
        serde_json::json!({
            "schema": "boole.metered-tuple-package-status.v1",
            "outcome": outcome,
            "reason": reason,
            "nonIssuable": true,
            "activationAllowed": false,
        }),
    )
}

fn read_bounded(path: &OsStr, cap: usize) -> Result<Vec<u8>, Response> {
    use std::os::unix::fs::OpenOptionsExt;
    let read_failed = || status("retryable_unavailable", "input_read_failed");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| read_failed())?;
    let metadata = file.metadata().map_err(|_| read_failed())?;
    if !metadata.is_file() {
        return Err(read_failed());
    }
    if metadata.len() > cap as u64 {
        return Err(status("input_error", "input_too_large"));
    }
    let mut bytes = Vec::new();
    file.take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| read_failed())?;
    if bytes.len() > cap {
        return Err(status("input_error", "input_too_large"));
    }
    Ok(bytes)
}

fn store(path: &OsStr) -> Result<LocalPackageStore, Response> {
    LocalPackageStore::open(
        Path::new(path),
        LocalPackageStoreConfig {
            enabled: true,
            ..Default::default()
        },
    )
    .map_err(|_| status("retryable_unavailable", "store_open_failed"))
}

fn package_error(error: ReverifyError) -> Response {
    match error {
        ReverifyError::Package(error) => status("package_error", &error.to_string()),
        ReverifyError::Store(_) => status("retryable_unavailable", "store_error"),
    }
}

fn expected_root(raw: &OsStr) -> Result<PackageRoot, Response> {
    raw.to_str()
        .and_then(|s| PackageRoot::from_hex(s).ok())
        .ok_or_else(|| status("input_error", "invalid_expected_root"))
}

fn run() -> Result<Response, Response> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    match args.first().and_then(|s| s.to_str()) {
        Some("pack") if args.len() == 3 => {
            let task = read_bounded(&args[1], MAX_TASK_BYTES)?;
            let answer = read_bounded(&args[2], MAX_ANSWER_BYTES)?;
            let package = build_package(&task, &answer)
                .map_err(|error| status("input_error", &error.to_string()))?;
            // stdout is the exact binary sidecar; stderr is local metadata, not
            // a signature or authority for an independently received root.
            eprintln!("packageRoot={}", package.root().to_hex());
            Ok((0, package.canonical_bytes().to_vec()))
        }
        Some(command @ ("import" | "restore")) if args.len() == 4 => {
            let root = expected_root(&args[2])?;
            let bytes = read_bounded(&args[3], MAX_METERED_PACKAGE_BYTES)?;
            if command == "restore" {
                restore_package(
                    Path::new(&args[1]),
                    LocalPackageStoreConfig {
                        enabled: true,
                        ..Default::default()
                    },
                    root,
                    &bytes,
                )
                .map_err(package_error)?;
            } else {
                let mut store = store(&args[1])?;
                import_package(&mut store, root, &bytes).map_err(package_error)?;
            }
            Ok(json(
                0,
                serde_json::json!({
                    "schema": "boole.metered-tuple-package-status.v1",
                    "outcome": if command == "restore" { "restored_or_present" } else { "stored" },
                    "packageRoot": root.to_hex(),
                    "nonIssuable": true,
                    "activationAllowed": false,
                }),
            ))
        }
        Some("verify") if args.len() == 3 => {
            let root = expected_root(&args[2])?;
            let mut store = store(&args[1])?;
            match reverify_stored(&mut store, root).map_err(package_error)? {
                StoredVerification::RetryableUnavailable => Ok(status(
                    "retryable_unavailable",
                    "package_missing_fetch_intent_durable",
                )),
                StoredVerification::Verified(result) => Ok(json(
                    u8::from(result.verdict() != Verdict::Accepted),
                    serde_json::json!({
                        "schema": "boole.metered-tuple-package-verification.v1",
                        "packageRoot": root.to_hex(),
                        "verification": serde_json::from_slice::<serde_json::Value>(&result.canonical_bytes())
                            .expect("canonical verifier JSON"),
                    }),
                )),
            }
        }
        _ => Err(status("input_error", USAGE)),
    }
}

fn main() -> ExitCode {
    let (code, bytes) = run().unwrap_or_else(|error| error);
    if std::io::stdout().lock().write_all(&bytes).is_err() {
        return ExitCode::from(2);
    }
    ExitCode::from(code)
}
