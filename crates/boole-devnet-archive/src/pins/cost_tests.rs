//! Reproducible synthetic pin-store cost, not an Engine/gate benchmark.
use super::*;
use std::time::Instant;
fn emit(
    root: &Path,
    manifest: &Manifest,
    sequence: &mut u64,
    last: &mut Hash,
    event: Event,
) -> Result<()> {
    let frame = Frame {
        version: 1,
        sequence: *sequence,
        previous: *last,
        event,
    };
    let raw = serde_json::to_vec(&frame)?;
    let digest = frame_hash(manifest, &raw)?;
    // Bulk fixture construction is outside the timed durable publication path.
    std::fs::write(
        root.join(format!("{:020}-{}.json", *sequence, hex::encode(digest))),
        raw,
    )?;
    *sequence += 1;
    *last = digest;
    Ok(())
}
#[test]
#[ignore = "explicit synthetic 64/1000/10000-height filesystem measurement"]
fn pin_cost_64_1000_10000() -> Result<()> {
    let Fixture {
        _dir,
        mut source,
        pins,
        request,
        ..
    } = fixture(true)?;
    for h in 5..=10005 {
        let parent =
            boole_derivation::attributes::hex_bytes(source.hash(h - 1).unwrap(), "parent")?
                .try_into()
                .unwrap();
        source.append(&empty_fixture(h, parent))?;
    }
    let root = source.root().join(DIR).join("records");
    let manifest = pins.view.manifest.clone();
    let mut sequence = pins.view.files.len() as u64;
    let mut last = pins.view.last;
    drop(pins);
    let mut current = 4;
    for target in [64, 1000, 10000] {
        for h in current + 1..=target {
            emit(
                &root,
                &manifest,
                &mut sequence,
                &mut last,
                Event::Prepared {
                    binding: Box::new(binding(&source, h)?),
                    candidates: vec![],
                    inputs: vec![],
                },
            )?;
            let b = binding(&source, h)?;
            emit(
                &root,
                &manifest,
                &mut sequence,
                &mut last,
                Event::Selected {
                    height: h,
                    hash: b.header.hash,
                    root: b.root,
                },
            )?;
        }
        PIN_COST.with(|c| c.set((0, 0, 0)));
        let open = Instant::now();
        let mut pins = PinStore::open(&source)?;
        let startup_ms = open.elapsed().as_secs_f64() * 1000.;
        let startup_records = PIN_COST.with(|c| c.get().0);
        assert!(startup_records >= target as usize * 2);
        let mut samples = vec![];
        for h in target + 1..=target + 5 {
            let b = binding(&source, h)?;
            PIN_COST.with(|c| c.set((0, 0, 0)));
            let start = Instant::now();
            pins.refresh(&source)?;
            pins.view()
                .verify(&source, std::slice::from_ref(&request), h - 1)?;
            pins.prepare(&source, b.clone(), std::slice::from_ref(&request))?;
            pins.select(h, &b.header.hash, b.root)?;
            let millis = start.elapsed().as_secs_f64() * 1000.;
            let (r, w, i) = PIN_COST.with(|c| c.get());
            assert_eq!((r, i), (0, 0), "per-height prefix read/index copy");
            assert!(w <= 1, "per-height row reads {w}");
            samples.push(serde_json::json!({"height":h,"ms":millis,"record_bodies_read":r,"rows_read":w,"index_entries_copied":i}));
        }
        println!(
            "PIN_COST {}",
            serde_json::json!({"schema":"boole-pin-cost-v1","seed_heights":target,"startup_ms":startup_ms,"startup_record_bodies":startup_records,"samples":samples,"scope":"writer refresh + verify + durable Prepared/Selected, one retained candidate, full startup excluded"})
        );
        current = target + 5;
        sequence = pins.view.files.len() as u64;
        last = pins.view.last;
    }
    Ok(())
}
