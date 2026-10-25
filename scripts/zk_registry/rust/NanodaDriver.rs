//! Four bounded native workers; results and aggregate debit commit in export order.
use crate::gate_fuel::{self, Limits, Scope, Snapshot, Stop};
use crate::util::ExportFile;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Condvar, Mutex,
};

#[derive(Clone, Debug, Serialize)]
pub struct Outcome {
    pub declaration_index: usize,
    pub code: u32,
    pub counters: Snapshot,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub version: u32,
    pub code: u32,
    pub workers: usize,
    pub worker_stack_bytes: Vec<u64>,
    pub checked_declarations: u64,
    pub selected_declarations: u64,
    pub aggregate_work: u64,
    pub aggregate_steps: u64,
    pub maximum_depth: u64,
    pub maximum_composed_depth: u64,
    pub peak_stack_bytes: u64,
    pub terminal: Option<Outcome>,
    /// Physical attempts, including canceled/in-flight declarations. Observation
    /// only; ordered aggregate_work is the deterministic result debit.
    pub attempted_work: u64,
    pub worker_last: Vec<(usize, Outcome)>,
}

impl Summary {
    pub fn unavailable() -> Self {
        Self {
            version: 1,
            code: 70,
            workers: 4,
            worker_stack_bytes: Vec::new(),
            checked_declarations: 0,
            selected_declarations: 0,
            aggregate_work: 0,
            aggregate_steps: 0,
            maximum_depth: 0,
            maximum_composed_depth: 0,
            peak_stack_bytes: 0,
            terminal: None,
            attempted_work: 0,
            worker_last: Vec::new(),
        }
    }
}

pub fn limits_valid(limits: Limits) -> bool {
    limits.counter_version == 1
        && limits.declaration_charge == 64
        && limits.worker_stack_bytes == 64 * 1024 * 1024
        && limits.literal_bytes > 0
        && limits.literal_bytes <= 4096
        && limits.steps_per_declaration <= 33_554_432
        && limits.depth_per_declaration <= 32_768
        && limits.work_per_declaration <= 34_359_738_368
        && limits.aggregate_work <= 34_359_738_368_000_000
}

fn caught_code(result: std::thread::Result<()>) -> u32 {
    match result {
        Ok(()) => 0,
        Err(error) => error.downcast_ref::<Stop>().map_or(4, |stop| stop.code()),
    }
}

#[cfg(test)]
thread_local! {
    static TEST_SPAWN_LIMIT: std::cell::Cell<usize> = const { std::cell::Cell::new(4) };
}

#[inline(always)]
fn spawn_allowed(worker: usize) -> bool {
    #[cfg(test)]
    {
        TEST_SPAWN_LIMIT.with(|limit| worker < limit.get())
    }
    #[cfg(not(test))]
    {
        let _ = worker;
        true
    }
}

struct Pool {
    next: usize,
    committed: usize,
    ready: Vec<(usize, u32, u64)>,
    go: bool,
    stop: bool,
    results: BTreeMap<usize, Outcome>,
    reservations: BTreeMap<usize, u64>,
    available: u64,
    attempted_work: u64,
    peak_stack: u64,
    worker_last: BTreeMap<usize, Outcome>,
}

/// The window has at most four declarations in flight. Each declaration's local
/// counters are independent of scheduling. Every in-flight check reserves its
/// full variable allowance. A smaller final allowance runs alone, after all
/// earlier results commit, so budget distribution cannot depend on scheduling.
pub fn check_export(
    export: &ExportFile<'_>,
    limits: Limits,
    observe: impl FnMut(&Outcome) -> std::io::Result<()>,
) -> Summary {
    check_selected_export(export, limits, None, observe)
}

/// Names use tagged components: s followed by a string, n followed by a u64.
/// The parent supplies this private selection only after certified-base identity.
fn selected_indices(
    export: &ExportFile<'_>,
    selection: Option<&[Vec<String>]>,
) -> Option<Vec<usize>> {
    let Some(selection) = selection else {
        return Some((0..export.declars.len()).collect());
    };
    let mut wanted: BTreeSet<_> = selection.iter().cloned().collect();
    if wanted.len() != selection.len() {
        return None;
    }
    let indices = export.with_ctx(|ctx| {
        let mut indices = Vec::new();
        for (index, (name, _)) in export.declars.iter().enumerate() {
            let mut name = *name;
            let mut parts = Vec::new();
            loop {
                match ctx.read_name(name) {
                    crate::name::Name::Anon => break,
                    crate::name::Name::Str(parent, string, _) => {
                        parts.push(format!("s{}", ctx.read_string(string)));
                        name = parent;
                    }
                    crate::name::Name::Num(parent, number, _) => {
                        parts.push(format!("n{number}"));
                        name = parent;
                    }
                }
            }
            parts.reverse();
            if wanted.remove(&parts) {
                indices.push(index);
            }
        }
        indices
    });
    wanted.is_empty().then_some(indices)
}

pub fn check_selected_export(
    export: &ExportFile<'_>,
    limits: Limits,
    selection: Option<&[Vec<String>]>,
    mut observe: impl FnMut(&Outcome) -> std::io::Result<()>,
) -> Summary {
    let mut summary = Summary::unavailable();
    if !limits_valid(limits) || export.config.num_threads != 4 {
        return summary;
    }
    let Ok(stack_bytes) = usize::try_from(limits.worker_stack_bytes) else {
        return summary;
    };
    let Some(indices) = selected_indices(export, selection) else {
        return summary;
    };
    let count = indices.len();
    summary.selected_declarations = count as u64;
    let precharge = match (count as u64).checked_mul(limits.declaration_charge) {
        Some(value) => value,
        None => {
            summary.code = 3;
            return summary;
        }
    };
    let cancel = AtomicBool::new(false);
    let state = Mutex::new(Pool {
        next: 0,
        committed: 0,
        ready: Vec::new(),
        go: false,
        stop: false,
        results: BTreeMap::new(),
        reservations: BTreeMap::new(),
        available: limits.aggregate_work.saturating_sub(precharge),
        attempted_work: precharge,
        peak_stack: 0,
        worker_last: BTreeMap::new(),
    });
    let changed = Condvar::new();
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for worker in 0..4 {
            let cancel = &cancel;
            let state = &state;
            let changed = &changed;
            let indices = &indices;
            let spawned = if spawn_allowed(worker) {
                std::thread::Builder::new()
                    .name(format!("nanoda-gate-{worker}"))
                    .stack_size(stack_bytes)
                    .spawn_scoped(scope, move || {
                        let ready_code = caught_code(catch_unwind(AssertUnwindSafe(|| {
                            let mut startup = limits;
                            startup.work_per_declaration = startup.work_per_declaration.max(64);
                            let _scope =
                                Scope::begin(startup, startup.work_per_declaration, cancel);
                        })));
                        let capacity = gate_fuel::snapshot().stack_capacity_bytes;
                        {
                            let mut pool = state.lock().unwrap();
                            pool.ready.push((worker, ready_code, capacity));
                            changed.notify_all();
                            while !pool.go && !pool.stop {
                                pool = changed.wait(pool).unwrap();
                            }
                            if pool.stop || ready_code != 0 {
                                return;
                            }
                        }
                        loop {
                            let (index, allowance) = {
                                let mut pool = state.lock().unwrap();
                                let maximum = limits
                                    .work_per_declaration
                                    .saturating_sub(limits.declaration_charge);
                                while !pool.stop
                                    && pool.next < count
                                    && (pool.next >= pool.committed + 4
                                        || pool.available < maximum
                                            && !pool.reservations.is_empty())
                                {
                                    pool = changed.wait(pool).unwrap();
                                }
                                if pool.stop || pool.next >= count {
                                    return;
                                }
                                let index = pool.next;
                                pool.next += 1;
                                let reservation = pool.available.min(maximum);
                                pool.available -= reservation;
                                pool.reservations.insert(index, reservation);
                                (index, reservation + limits.declaration_charge)
                            };
                            let code = caught_code(catch_unwind(AssertUnwindSafe(|| {
                                let _scope = Scope::begin(limits, allowance, cancel);
                                let (_, declaration) =
                                    export.declars.get_index(indices[index]).unwrap();
                                export.check_declar(declaration);
                            })));
                            let counters = gate_fuel::snapshot();
                            let mut pool = state.lock().unwrap();
                            pool.attempted_work = pool.attempted_work.saturating_add(
                                counters.work.saturating_sub(limits.declaration_charge),
                            );
                            pool.peak_stack = pool.peak_stack.max(counters.peak_stack_bytes);
                            let outcome = Outcome {
                                declaration_index: indices[index],
                                code,
                                counters,
                            };
                            pool.worker_last.insert(worker, outcome.clone());
                            if pool.stop {
                                return;
                            }
                            pool.results.insert(index, outcome);
                            changed.notify_all();
                        }
                    })
            } else {
                Err(std::io::Error::other("generated worker spawn failure"))
            };
            match spawned {
                Ok(handle) => handles.push(handle),
                Err(_) => {
                    state.lock().unwrap().stop = true;
                    changed.notify_all();
                    break;
                }
            }
        }
        let all_spawned = handles.len() == 4;
        if all_spawned {
            let mut pool = state.lock().unwrap();
            while pool.ready.len() < 4 {
                pool = changed.wait(pool).unwrap();
            }
            pool.ready.sort_by_key(|row| row.0);
            summary.worker_stack_bytes = pool.ready.iter().map(|row| row.2).collect();
            let all_ready = pool.ready.iter().all(|row| row.1 == 0);
            if !all_ready {
                pool.stop = true;
                changed.notify_all();
            } else {
                summary.code = 0;
                summary.aggregate_work = precharge;
                if precharge > limits.aggregate_work {
                    summary.code = 3;
                    pool.stop = true;
                } else {
                    pool.go = true;
                }
                changed.notify_all();
                drop(pool);
                for index in 0..count {
                    if summary.code != 0 {
                        break;
                    }
                    let mut pool = state.lock().unwrap();
                    while !pool.results.contains_key(&index) {
                        pool = changed.wait(pool).unwrap();
                    }
                    let mut outcome = pool.results.remove(&index).unwrap();
                    drop(pool);
                    let counters = &outcome.counters;
                    summary.maximum_depth = summary.maximum_depth.max(counters.depth);
                    summary.maximum_composed_depth =
                        summary.maximum_composed_depth.max(counters.composed_depth);
                    summary.peak_stack_bytes =
                        summary.peak_stack_bytes.max(counters.peak_stack_bytes);
                    let additional = counters.work.saturating_sub(limits.declaration_charge);
                    match summary.aggregate_work.checked_add(additional) {
                        Some(work) => {
                            summary.aggregate_work = work;
                            if work > limits.aggregate_work && outcome.code == 0 {
                                outcome.code = 3;
                            }
                        }
                        None => {
                            if outcome.code == 0 {
                                outcome.code = 3;
                            }
                        }
                    }
                    match summary.aggregate_steps.checked_add(counters.steps) {
                        Some(steps) => summary.aggregate_steps = steps,
                        None => {
                            if outcome.code == 0 {
                                outcome.code = 3;
                            }
                        }
                    }
                    if observe(&outcome).is_err() {
                        outcome.code = 70;
                    }
                    if outcome.code != 0 {
                        summary.code = outcome.code;
                        summary.terminal = Some(outcome);
                        break;
                    }
                    summary.checked_declarations += 1;
                    let mut pool = state.lock().unwrap();
                    let reservation = pool.reservations.remove(&index).unwrap();
                    // A successful declaration never consumes more than its reservation.
                    let unused = reservation
                        .checked_sub(additional)
                        .expect("native reservation invariant");
                    pool.available = pool
                        .available
                        .checked_add(unused)
                        .expect("native budget invariant");
                    pool.committed = index + 1;
                    changed.notify_all();
                }
            }
        }
        cancel.store(true, Ordering::Relaxed);
        {
            state.lock().unwrap().stop = true;
            changed.notify_all();
        }
        for handle in handles {
            if handle.join().is_err() {
                summary.code = 70;
            }
        }
        let pool = state.lock().unwrap();
        summary.attempted_work = pool.attempted_work;
        summary.peak_stack_bytes = summary.peak_stack_bytes.max(pool.peak_stack);
        summary.worker_last = pool
            .worker_last
            .iter()
            .map(|(worker, outcome)| (*worker, outcome.clone()))
            .collect();
    });
    summary
}

#[cfg(test)]
pub(crate) mod gate_tests {
    use super::*;
    use crate::util::Config;
    use serde_json::json;

    pub(crate) fn generated_export(count: usize) -> ExportFile<'static> {
        let mut rows = vec![
            json!({"meta":{"exporter":{"name":"lean4export","version":"3.1.0"},"format":{"version":"3.1.0"},"lean":{"githash":"5045d0056413266e57c625dcd7c365b10e377c52","version":"4.34.1"}}}),
            json!({"ie":0,"sort":0}),
        ];
        let names: Vec<_> = (0..count).map(|i| format!("Generated{i}")).collect();
        for (index, name) in names.iter().enumerate() {
            rows.push(json!({"in":index+1,"str":{"pre":0,"str":name}}));
            rows.push(json!({"axiom":{"name":index+1,"type":0,"levelParams":[],"isUnsafe":false}}));
        }
        let raw = rows
            .iter()
            .map(|row| row.to_string() + "\n")
            .collect::<String>();
        let config:Config=serde_json::from_value(json!({"use_stdin":true,"num_threads":4,"nat_extension":true,"string_extension":true,"permitted_axioms":names,"gate_limits":Limits::default()})).unwrap();
        let (export, skipped) =
            crate::parser::parse_export_file(std::io::Cursor::new(raw), config).unwrap();
        assert!(skipped.is_empty());
        export
    }

    fn deterministic(summary: &Summary) -> serde_json::Value {
        json!({"code":summary.code,"checked":summary.checked_declarations,"work":summary.aggregate_work,"steps":summary.aggregate_steps,"depth":summary.maximum_depth,"composed":summary.maximum_composed_depth,
            "terminal":summary.terminal.as_ref().map(|o|json!({"index":o.declaration_index,"code":o.code,"work":o.counters.work,"steps":o.counters.steps,"calls":o.counters.calls,"events":o.counters.events,"depth":o.counters.depth,"composed":o.counters.composed_depth}))})
    }

    #[test]
    fn gate_results_commit_in_export_order_on_four_native_workers() {
        let export = generated_export(8);
        let mut first = Vec::new();
        let mut second = Vec::new();
        let a = check_export(&export, Limits::default(), |row| {
            first.push(row.declaration_index);
            Ok(())
        });
        let b = check_export(&export, Limits::default(), |row| {
            std::thread::sleep(std::time::Duration::from_millis(1));
            second.push(row.declaration_index);
            Ok(())
        });
        assert_eq!(a.code, 0);
        assert_eq!(a.checked_declarations, 8);
        assert_eq!(first, (0..8).collect::<Vec<_>>());
        assert_eq!(first, second);
        assert_eq!(deterministic(&a), deterministic(&b));
        assert_eq!(a.worker_stack_bytes.len(), 4);
        assert!(a
            .worker_stack_bytes
            .iter()
            .all(|&bytes| bytes >= 64 * 1024 * 1024));
    }

    #[test]
    fn gate_partial_allowance_has_the_same_terminal_record() {
        let export = generated_export(8);
        let mut work = Vec::new();
        let baseline = check_export(&export, Limits::default(), |row| {
            work.push(row.counters.work);
            Ok(())
        });
        assert_eq!(baseline.code, 0);
        assert!(work.iter().all(|&value| value == work[0]));
        let variable = work[0] - 64;
        assert!(variable >= 2);
        let limits = Limits {
            work_per_declaration: work[0],
            aggregate_work: 8 * 64 + 2 * variable + variable / 2,
            ..Limits::default()
        };
        let a = check_export(&export, limits, |_| Ok(()));
        let b = check_export(&export, limits, |_| {
            std::thread::sleep(std::time::Duration::from_millis(1));
            Ok(())
        });
        assert_eq!(a.code, 3);
        assert_eq!(a.checked_declarations, 2);
        assert_eq!(a.terminal.as_ref().unwrap().declaration_index, 2);
        assert_eq!(deterministic(&a), deterministic(&b));
    }

    #[test]
    fn gate_worker_spawn_failures_are_unavailable() {
        let export = generated_export(8);
        for permitted in [0, 2] {
            TEST_SPAWN_LIMIT.with(|limit| limit.set(permitted));
            let result = check_export(&export, Limits::default(), |_| Ok(()));
            TEST_SPAWN_LIMIT.with(|limit| limit.set(4));
            assert_eq!(result.code, 70);
            assert_eq!(result.checked_declarations, 0);
        }
    }

    #[test]
    fn gate_status_io_failure_is_unavailable_and_driver_fault_stays_distinct() {
        let export = generated_export(1);
        let result = check_export(&export, Limits::default(), |_| {
            Err(std::io::Error::other("generated status IO failure"))
        });
        assert_eq!(result.code, 70);
        assert_eq!(
            caught_code(catch_unwind(|| panic!("generated driver invariant"))),
            4
        );
        assert_eq!(
            caught_code(catch_unwind(|| std::panic::panic_any(Stop::Reject))),
            2
        );
        assert_eq!(
            caught_code(catch_unwind(|| std::panic::panic_any(Stop::Cap))),
            3
        );
    }

    #[test]
    fn gate_depth_cap_is_typed_on_four_native_workers() {
        let cancel = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..4 {
                let cancel = &cancel;
                handles.push(
                    std::thread::Builder::new()
                        .stack_size(64 * 1024 * 1024)
                        .spawn_scoped(scope, move || {
                            let limits = Limits {
                                depth_per_declaration: 2,
                                ..Limits::default()
                            };
                            let code = caught_code(catch_unwind(AssertUnwindSafe(|| {
                                let _scope =
                                    Scope::begin(limits, limits.work_per_declaration, cancel);
                                gate_fuel::with_state(|fuel| {
                                    let _a = fuel.enter(0);
                                    let _b = fuel.enter(1);
                                    let _c = fuel.enter(2);
                                });
                            })));
                            let counters = gate_fuel::snapshot();
                            (code, counters.depth, counters.stack_capacity_bytes)
                        })
                        .unwrap(),
                );
            }
            for handle in handles {
                let (code, depth, capacity) = handle.join().unwrap();
                assert_eq!(code, 3);
                assert_eq!(depth, 3);
                assert!(capacity >= 64 * 1024 * 1024);
            }
        });
    }
}

/// Flat export DAGs use only preceding indices. Check height before recursive
/// name/universe/term helpers, without expanding shared subgraphs.
pub(crate) fn preflight_dag(dag: &crate::util::LeanDag<'_>, limits: Limits) {
    use crate::{expr::Expr, level::Level, name::Name};
    use std::panic::panic_any;
    let bound = |value: u64| {
        if value > limits.depth_per_declaration {
            panic_any(Stop::Cap);
        }
        value
    };
    let mut names = Vec::<u64>::with_capacity(dag.names.len());
    for name in &dag.names {
        let height = match name {
            Name::Anon => 1,
            Name::Str(p, ..) | Name::Num(p, ..) => 1 + names[p.idx()],
        };
        names.push(bound(height));
    }
    let mut levels = Vec::<u64>::with_capacity(dag.levels.len());
    for level in &dag.levels {
        let height = match level {
            Level::Zero | Level::Param(..) => 1,
            Level::Succ(p, ..) => 1 + levels[p.idx()],
            Level::Max(a, b, ..) | Level::IMax(a, b, ..) => {
                1 + levels[a.idx()].max(levels[b.idx()])
            }
        };
        levels.push(bound(height));
    }
    let mut exprs = Vec::<u64>::with_capacity(dag.exprs.len());
    for expr in &dag.exprs {
        let height = match expr {
            Expr::App { fun, arg, .. } => 1 + exprs[fun.idx()].max(exprs[arg.idx()]),
            Expr::Pi {
                binder_type, body, ..
            }
            | Expr::Lambda {
                binder_type, body, ..
            } => 1 + exprs[binder_type.idx()].max(exprs[body.idx()]),
            Expr::Let {
                binder_type,
                val,
                body,
                ..
            } => {
                1 + exprs[binder_type.idx()]
                    .max(exprs[val.idx()])
                    .max(exprs[body.idx()])
            }
            Expr::Proj { structure, .. } => 1 + exprs[structure.idx()],
            Expr::Local { binder_type, .. } => 1 + exprs[binder_type.idx()],
            _ => 1,
        };
        exprs.push(bound(height));
    }
    for string in &dag.strings {
        if string.len() as u64 > limits.literal_bytes {
            panic_any(Stop::Cap);
        }
    }
    if let Some(numbers) = &dag.bignums {
        for number in numbers {
            if number.bits().div_ceil(8) > limits.literal_bytes {
                panic_any(Stop::Cap);
            }
        }
    }
}
