use anyhow::Result;
use boole_gate_supervisor::real_deadline::Deadline;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use boole_gate_supervisor::{
    codec::{digest, Scratch},
    real_decoder::Config,
    real_descriptor::Descriptor,
    real_reply::Context,
};
#[test]
fn preparation_deadline_stops_a_slow_reader_between_bounded_reads() -> Result<()> {
    use std::{
        io::Read,
        time::{Duration, Instant},
    };
    struct Slow {
        reads: usize,
    }
    impl Read for Slow {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.reads += 1;
            std::thread::sleep(Duration::from_millis(30));
            b[0] = 1;
            Ok(1)
        }
    }
    let mut s = Slow { reads: 0 };
    let mut b = [0; 65536];
    let start = Instant::now();
    let deadline = Deadline::after(10);
    let error = loop {
        match deadline.read(&mut s, &mut b) {
            Ok(_) => continue,
            Err(e) => break e,
        }
    };
    assert!(error.is::<boole_gate_supervisor::real_deadline::Expired>());
    assert_eq!(
        s.reads, 1,
        "deadline was reset or checked only after complete preparation"
    );
    assert!(start.elapsed() < Duration::from_millis(250));
    Ok(())
}
#[test]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn native_typed_decoder_separates_checksum_from_a_real_output_write_fault() -> Result<()> {
    let Some(raw_config) = std::env::var_os("BOOLE_M3_REAL_DECODER") else {
        return Ok(());
    };
    let config: Config = serde_json::from_slice(&std::fs::read(raw_config)?)?;
    let d = Descriptor::decode(include_bytes!(
        "../../../fixtures/m3-real-gate/descriptor-v1.json"
    ))?;
    let fixture: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/m3-real-gate/reply-cases.json"
    ))?;
    let mut c: Context = serde_json::from_value(fixture["context"].clone())?;
    let raw = b"public codec fixture".repeat(90);
    c.export_digest = hex::encode(digest(&raw));
    let root = Scratch::new(&std::env::temp_dir())?;
    let codec = boole_gate_supervisor::codec::Codec::new(config.executable.clone())?;
    let frame = codec.compress(&raw, &root.0)?;
    for mode in ["ok", "write", "checksum", "digest"] {
        let scratch = Scratch::new(&root.0)?;
        let mut frame = frame.clone();
        if mode == "write" {
            std::fs::create_dir(scratch.0.join("decoded.bundle"))?;
        }
        if mode == "checksum" {
            *frame.last_mut().unwrap() ^= 1;
        }
        let mut context = c.clone();
        if mode == "digest" {
            context.export_digest = hex::encode(digest(b"miner committed a different export"));
        }
        let out = config.decode(
            &frame,
            raw.len() as u64,
            &d,
            &context,
            std::path::Path::new("/usr/bin/python3"),
            &scratch.0,
            Deadline::after(5000),
        )?;
        match mode {
            "ok" => {
                assert_eq!(out.raw.as_deref(), Some(raw.as_slice()));
                assert_eq!(out.status.status, "ok");
            }
            "write" => {
                assert_eq!(
                    (&out.status.status[..], &out.status.cause[..]),
                    ("unavailable", "write")
                );
                assert!(out.raw.is_none());
            }
            "checksum" => {
                assert_eq!(
                    (&out.status.status[..], &out.status.cause[..]),
                    ("invalid-data", "checksum")
                );
                assert!(out.raw.is_none());
            }
            _ => {
                assert_eq!(
                    (&out.status.status[..], &out.status.cause[..]),
                    ("invalid-data", "digest")
                );
                assert!(out.raw.is_none() && out.path.is_none());
                assert!(!scratch.0.join("decoded.bundle").exists());
            }
        }
    }
    println!("REAL-DECODER native=true positive=true checksum=true write-fault=true");
    Ok(())
}
