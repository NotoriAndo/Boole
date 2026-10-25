use nanoda_lib::{
    gate_driver::{self, Summary},
    gate_fuel::Stop,
    util::Config,
};
use std::io::{BufWriter, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

fn run(path: &Path) -> Summary {
    let config = match catch_unwind(AssertUnwindSafe(|| Config::try_from(path))) {
        Ok(Ok(config)) => config,
        _ => return Summary::unavailable(),
    };
    let limits = match config.gate_limits {
        Some(limits) if gate_driver::limits_valid(limits) && config.num_threads == 4 => limits,
        _ => return Summary::unavailable(),
    };
    let mut vectors = match std::env::var_os("NANODA_VECTOR_FILE") {
        Some(path) => match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
        {
            Ok(file) => Some(BufWriter::new(file)),
            Err(_) => return Summary::unavailable(),
        },
        None => None,
    };
    let parsed = catch_unwind(AssertUnwindSafe(|| config.to_export_file()));
    let export = match parsed {
        Ok(Ok((export, skipped))) if skipped.is_empty() => export,
        // The gate, not the miner, produces this export and configuration.
        // Skipped/unpermitted axioms, semver and literal-setting disagreement,
        // typed IO/serde syntax/EOF and other decoder failures are unavailable.
        // Only the decoder's explicit measured structural cap keeps exit 3.
        Ok(Ok(_)) | Ok(Err(_)) => return Summary::unavailable(),
        Err(error) => {
            let mut summary = Summary::unavailable();
            summary.code = decoder_panic_code(error.downcast_ref::<Stop>());
            return summary;
        }
    };
    let check_started = std::time::Instant::now();
    let mut summary = gate_driver::check_selected_export(
        &export,
        limits,
        export.config.checked_declaration_names.as_deref(),
        |outcome| {
            if let Some(writer) = vectors.as_mut() {
                serde_json::to_writer(&mut *writer, outcome)?;
                writer.write_all(b"\n")?;
            }
            Ok(())
        },
    );
    // Optional private calibration telemetry. It never enters a counter vector,
    // a consensus debit or a production acceptance decision.
    if let Some(path) = std::env::var_os("NANODA_TIMING_FILE") {
        let write = || -> std::io::Result<()> {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            serde_json::to_writer(
                file,
                &serde_json::json!({
                    "check_ns": u64::try_from(check_started.elapsed().as_nanos()).unwrap_or(u64::MAX)
                }),
            )?;
            Ok(())
        };
        if write().is_err() {
            summary.code = 70;
        }
    }
    if let Some(writer) = vectors.as_mut() {
        if writer
            .flush()
            .and_then(|()| writer.get_ref().sync_all())
            .is_err()
        {
            summary.code = 70;
        }
    }
    summary
}

fn decoder_panic_code(stop: Option<&Stop>) -> u32 {
    match stop {
        Some(Stop::Cap) => 3,
        _ => 70,
    }
}

fn publish(summary: &Summary) -> std::io::Result<()> {
    let path = std::env::var_os("NANODA_STATUS_FILE")
        .ok_or_else(|| std::io::Error::other("status path required"))?;
    publish_to(Path::new(&path), summary)
}

fn publish_to(path: &Path, summary: &Summary) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != 0 || metadata.nlink() != 1 {
        return Err(std::io::Error::other(
            "exclusive empty status file required",
        ));
    }
    serde_json::to_writer(&mut file, summary)?;
    file.write_all(b"\n")?;
    file.sync_all()
}

fn finish_load(
    spawned: std::io::Result<std::thread::JoinHandle<std::thread::Result<Summary>>>,
) -> Summary {
    match spawned {
        Err(_) => Summary::unavailable(),
        Ok(handle) => match handle.join() {
            Err(_) => Summary::unavailable(),
            Ok(Ok(summary)) => summary,
            Ok(Err(error)) => {
                let mut summary = Summary::unavailable();
                summary.code = error.downcast_ref::<Stop>().map_or(4, |stop| stop.code());
                summary
            }
        },
    }
}

fn main() {
    std::panic::set_hook(Box::new(|_| eprintln!("nanoda trusted failure")));
    let args: Vec<_> = std::env::args_os().collect();
    let summary = if args.len() == 2 {
        finish_load(
            std::thread::Builder::new()
                .name("nanoda-load".into())
                .stack_size(64 * 1024 * 1024)
                .spawn(move || catch_unwind(AssertUnwindSafe(|| run(Path::new(&args[1]))))),
        )
    } else {
        Summary::unavailable()
    };
    let code = if publish(&summary).is_ok() {
        summary.code
    } else {
        70
    };
    std::process::exit(code as i32);
}

#[cfg(test)]
mod gate_tests {
    use super::*;

    #[test]
    fn gate_generated_decode_faults_are_unavailable() {
        assert_eq!(decoder_panic_code(None), 70);
        for stop in [Stop::Reject, Stop::Fault, Stop::Unavailable] {
            assert_eq!(decoder_panic_code(Some(&stop)), 70);
        }
        assert_eq!(decoder_panic_code(Some(&Stop::Cap)), 3);
    }

    #[test]
    fn gate_load_spawn_and_join_failures_are_unavailable() {
        let failed = Err(std::io::Error::from_raw_os_error(libc::EAGAIN));
        assert_eq!(finish_load(failed).code, 70);
        let panicked = std::thread::spawn(|| -> std::thread::Result<Summary> {
            panic!("generated join failure")
        });
        assert_eq!(finish_load(Ok(panicked)).code, 70);
    }

    #[test]
    fn gate_typed_load_outcomes_keep_their_codes() {
        for (stop, expected) in [
            (Stop::Cap, 3),
            (Stop::Reject, 2),
            (Stop::Fault, 4),
            (Stop::Unavailable, 70),
        ] {
            let handle = std::thread::spawn(move || catch_unwind(|| std::panic::panic_any(stop)));
            assert_eq!(finish_load(Ok(handle)).code, expected);
        }
    }

    #[test]
    fn gate_status_open_refuses_symlinks_on_the_native_target() {
        use std::os::unix::fs::symlink;
        let directory =
            std::env::temp_dir().join(format!("boole-nanoda-nofollow-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let canary = directory.join("canary");
        let link = directory.join("status");
        std::fs::write(&canary, []).unwrap();
        symlink(&canary, &link).unwrap();
        let before = std::fs::metadata(&canary).unwrap().modified().unwrap();
        assert!(publish_to(&link, &Summary::unavailable()).is_err());
        assert_eq!(std::fs::read(&canary).unwrap(), b"");
        assert_eq!(
            std::fs::metadata(&canary).unwrap().modified().unwrap(),
            before
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
