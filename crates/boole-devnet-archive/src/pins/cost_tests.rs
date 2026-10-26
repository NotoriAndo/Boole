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
// All nine cells use the same public durable prepare/select path. Historical
// construction is synthetic and outside timing; candidate ranges span history.
fn cost_case(target: u64, retained: usize) -> Result<()> {
    let Fixture {
        _dir,
        mut source,
        pins,
        request,
        ..
    } = fixture(false)?;
    for h in 5..=target + 5 {
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
    for h in 0..=target {
        let b = binding(&source, h)?;
        emit(
            &root,
            &manifest,
            &mut sequence,
            &mut last,
            Event::Prepared {
                binding: Box::new(b.clone()),
                candidates: vec![],
                inputs: vec![],
            },
        )?;
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
    let mut requests = vec![];
    let mut ranges = IntervalUnion::default();
    for i in 0..retained {
        let mut r = request.clone();
        r.key.1 = super::hash(&(i as u64).to_be_bytes());
        let first = 1 + (i as u64 * (target - 3)) / retained as u64;
        r.commit_height = first;
        r.open_height = first + 1;
        r.completion_height = first + 2;
        r.reveal_height = first;
        ranges.extend(&r.intervals());
        requests.push(r);
    }
    let mut inputs = vec![];
    for (lo, hi) in ranges.0 {
        for h in lo..=hi {
            inputs.push((h, row_hash(&source.row(h)?)?));
        }
    }
    emit(
        &root,
        &manifest,
        &mut sequence,
        &mut last,
        Event::Prepared {
            binding: Box::new(binding(&source, target)?),
            candidates: requests.clone(),
            inputs,
        },
    )?;
    PIN_COST.with(|c| c.set((0, 0, 0)));
    let open = Instant::now();
    let mut pins = PinStore::open(&source)?;
    pins.view().verify(&source, &requests, target)?;
    let startup_ms = open.elapsed().as_secs_f64() * 1000.;
    let startup_records = PIN_COST.with(|c| c.get().0);
    assert!(startup_records >= target as usize * 2);
    assert_eq!(pins.view.index.candidates.len(), retained);
    let mut samples = vec![];
    for h in target + 1..=target + 5 {
        let b = binding(&source, h)?;
        PIN_COST.with(|c| c.set((0, 0, 0)));
        PIN_ROW_VISITS.with(|c| c.set(0));
        crate::journal::CHECK_COUNT.with(|c| c.set(0));
        let start = Instant::now();
        pins.refresh(&source)?;
        pins.view().verify(&source, &requests, h - 1)?;
        pins.prepare(&source, b.clone(), &requests)?;
        pins.select(h, &b.header.hash, b.root)?;
        let millis = start.elapsed().as_secs_f64() * 1000.;
        let (r, w, i) = PIN_COST.with(|c| c.get());
        assert_eq!((r, i), (0, 0), "per-height prefix read/index copy");
        assert!(w <= 1, "per-height raw row reads {w}");
        samples.push(serde_json::json!({"height":h,"ms":millis,"record_bodies_read":r,"rows_read":w,"row_visits":PIN_ROW_VISITS.with(|c|c.get()),"journal_checks":crate::journal::CHECK_COUNT.with(|c|c.get()),"index_entries_copied":i}));
    }
    println!(
        "PIN_COST {}",
        serde_json::json!({"schema":"boole-pin-cost-v2","seed_heights":target,"retained_candidates":retained,"startup_ms":startup_ms,"startup_record_bodies":startup_records,"samples":samples,"scope":"writer refresh + verify + durable Prepared/Selected; full startup and initial candidate coverage verification excluded"})
    );
    Ok(())
}
#[test]
#[ignore = "explicit synthetic 3x3 height/candidate filesystem measurement"]
fn pin_cost_64_1000_10000() -> Result<()> {
    for retained in [1, 100, 1000] {
        for target in [64, 1000, 10000] {
            cost_case(target, retained)?;
        }
    }
    Ok(())
}
