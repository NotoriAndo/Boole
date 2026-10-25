//! Deterministic nanoda accounting. Timing and stack bytes are observations only.
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::marker::PhantomData;
use std::panic::panic_any;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub counter_version: u32,
    pub steps_per_declaration: u64,
    pub depth_per_declaration: u64,
    pub work_per_declaration: u64,
    pub aggregate_work: u64,
    pub declaration_charge: u64,
    pub literal_bytes: u64,
    pub worker_stack_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            counter_version: 1,
            steps_per_declaration: 33_554_432,
            depth_per_declaration: 32_768,
            work_per_declaration: 34_359_738_368,
            aggregate_work: 34_359_738_368_000_000,
            declaration_charge: 64,
            literal_bytes: 4096,
            worker_stack_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Cap,
    Reject,
    Fault,
    Unavailable,
    Canceled,
}

impl Stop {
    pub fn code(self) -> u32 {
        match self {
            Self::Cap => 3,
            Self::Reject => 2,
            Self::Fault => 4,
            Self::Unavailable | Self::Canceled => 70,
        }
    }
}

/// A semantic check in the pinned kernel rejected its input. Driver invariants
/// continue to use ordinary panics and retain the separate fault outcome.
pub(crate) fn reject() -> ! {
    panic_any(Stop::Reject)
}

#[macro_export]
macro_rules! gate_reject {
    () => { $crate::gate_fuel::reject() };
    ($($message:tt)+) => {{
        let _ = format_args!($($message)+);
        $crate::gate_fuel::reject()
    }};
}

#[macro_export]
macro_rules! gate_assert {
    ($condition:expr $(,)?) => {
        if !$condition { $crate::gate_reject!() }
    };
    ($condition:expr, $($message:tt)+) => {
        if !$condition { $crate::gate_reject!($($message)+) }
    };
}

#[macro_export]
macro_rules! gate_assert_eq {
    ($left:expr, $right:expr $(,)?) => {
        match (&$left, &$right) {
            (left, right) => if !(*left == *right) { $crate::gate_reject!() }
        }
    };
    ($left:expr, $right:expr, $($message:tt)+) => {
        match (&$left, &$right) {
            (left, right) => if !(*left == *right) { $crate::gate_reject!($($message)+) }
        }
    };
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Snapshot {
    pub calls: [u64; 3],
    pub events: [u64; 8],
    pub steps: u64,
    pub work: u64,
    pub depth: u64,
    pub remaining_depth: u64,
    pub stack_capacity_bytes: u64,
    pub peak_stack_bytes: u64,
    pub composed_depth: u64,
}

pub struct State {
    enabled: Cell<bool>,
    limits: Cell<Limits>,
    allowance: Cell<u64>,
    calls: [Cell<u64>; 3],
    events: [Cell<u64>; 8],
    steps: Cell<u64>,
    work: Cell<u64>,
    depth: Cell<u64>,
    maximum_depth: Cell<u64>,
    helper_depth: Cell<u64>,
    maximum_composed_depth: Cell<u64>,
    stack_top: Cell<usize>,
    stack_capacity: Cell<usize>,
    peak_stack: Cell<usize>,
    cancel: Cell<*const AtomicBool>,
}

impl State {
    const fn new() -> Self {
        Self {
            enabled: Cell::new(false),
            limits: Cell::new(Limits {
                counter_version: 1,
                steps_per_declaration: 0,
                depth_per_declaration: 0,
                work_per_declaration: 0,
                aggregate_work: 0,
                declaration_charge: 0,
                literal_bytes: 0,
                worker_stack_bytes: 0,
            }),
            allowance: Cell::new(0),
            calls: [const { Cell::new(0) }; 3],
            events: [const { Cell::new(0) }; 8],
            steps: Cell::new(0),
            work: Cell::new(0),
            depth: Cell::new(0),
            maximum_depth: Cell::new(0),
            stack_top: Cell::new(0),
            helper_depth: Cell::new(0),
            maximum_composed_depth: Cell::new(0),
            stack_capacity: Cell::new(0),
            peak_stack: Cell::new(0),
            cancel: Cell::new(std::ptr::null()),
        }
    }

    #[inline(always)]
    fn poll(&self) {
        let pointer = self.cancel.get();
        // Scope owns the borrow until Drop disables accounting and clears this pointer.
        if !pointer.is_null() && unsafe { &*pointer }.load(Ordering::Relaxed) {
            panic_any(Stop::Canceled);
        }
    }

    #[inline(always)]
    fn add_work(&self, amount: u64) {
        let next = self
            .work
            .get()
            .checked_add(amount)
            .unwrap_or_else(|| panic_any(Stop::Cap));
        self.work.set(next);
        if next > self.allowance.get() {
            panic_any(Stop::Cap);
        }
    }

    /// Attempts, not successful allocations: unrelated global hash-cons hits earn no discount.
    #[inline(always)]
    pub fn event(&self, kind: usize, amount: u64) {
        if !self.enabled.get() {
            return;
        }
        self.poll();
        let next = self.events[kind]
            .get()
            .checked_add(amount)
            .unwrap_or_else(|| panic_any(Stop::Cap));
        self.events[kind].set(next);
        let weight = if kind == 3 { 8 } else { 1 };
        self.add_work(
            amount
                .checked_mul(weight)
                .unwrap_or_else(|| panic_any(Stop::Cap)),
        );
    }

    #[inline(always)]
    pub fn enter(&self, kind: usize) -> Probe<'_> {
        if !self.enabled.get() {
            return Probe(None);
        }
        self.poll();
        let calls = self.calls[kind]
            .get()
            .checked_add(1)
            .unwrap_or_else(|| panic_any(Stop::Cap));
        self.calls[kind].set(calls);
        let steps = self
            .steps
            .get()
            .checked_add(1)
            .unwrap_or_else(|| panic_any(Stop::Cap));
        self.steps.set(steps);
        let depth = self
            .depth
            .get()
            .checked_add(1)
            .unwrap_or_else(|| panic_any(Stop::Cap));
        self.depth.set(depth);
        self.maximum_depth.set(self.maximum_depth.get().max(depth));
        let guard = Probe(Some(self));
        let used = self.stack_top.get().saturating_sub(stack_pointer());
        self.peak_stack.set(self.peak_stack.get().max(used));
        let limits = self.limits.get();
        let composed = depth
            .checked_add(self.helper_depth.get())
            .unwrap_or_else(|| panic_any(Stop::Cap));
        self.maximum_composed_depth
            .set(self.maximum_composed_depth.get().max(composed));
        if steps > limits.steps_per_declaration || composed > limits.depth_per_declaration {
            panic_any(Stop::Cap);
        }
        guard
    }

    /// Guard recursive substitution and universe/name helpers without changing
    /// the three historical step counters or the work-event definition.
    #[inline(always)]
    pub fn helper(&self) -> Helper<'_> {
        if !self.enabled.get() {
            return Helper(None);
        }
        self.poll();
        let depth = self
            .helper_depth
            .get()
            .checked_add(1)
            .unwrap_or_else(|| panic_any(Stop::Cap));
        self.helper_depth.set(depth);
        let guard = Helper(Some(self));
        let composed = depth
            .checked_add(self.depth.get())
            .unwrap_or_else(|| panic_any(Stop::Cap));
        self.maximum_composed_depth
            .set(self.maximum_composed_depth.get().max(composed));
        let used = self.stack_top.get().saturating_sub(stack_pointer());
        self.peak_stack.set(self.peak_stack.get().max(used));
        if composed > self.limits.get().depth_per_declaration {
            panic_any(Stop::Cap);
        }
        guard
    }

    pub fn bound_literal(&self, bytes: u64) {
        if self.enabled.get() && bytes > self.limits.get().literal_bytes {
            panic_any(Stop::Cap);
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            calls: std::array::from_fn(|i| self.calls[i].get()),
            events: std::array::from_fn(|i| self.events[i].get()),
            steps: self.steps.get(),
            work: self.work.get(),
            depth: self.maximum_depth.get(),
            remaining_depth: self.depth.get(),
            stack_capacity_bytes: self.stack_capacity.get() as u64,
            peak_stack_bytes: self.peak_stack.get() as u64,
            composed_depth: self.maximum_composed_depth.get(),
        }
    }
}

thread_local! { static STATE: State = const { State::new() }; }
pub fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(f)
}
pub fn snapshot() -> Snapshot {
    with_state(State::snapshot)
}

pub struct Probe<'a>(Option<&'a State>);
impl Drop for Probe<'_> {
    fn drop(&mut self) {
        if let Some(state) = self.0 {
            state.depth.set(state.depth.get() - 1);
        }
    }
}
pub struct Helper<'a>(Option<&'a State>);
impl Drop for Helper<'_> {
    fn drop(&mut self) {
        if let Some(state) = self.0 {
            state.helper_depth.set(state.helper_depth.get() - 1);
        }
    }
}

pub struct Scope<'a> {
    _borrow: PhantomData<&'a AtomicBool>,
}
impl Scope<'_> {
    pub fn begin(limits: Limits, allowance: u64, cancel: &AtomicBool) -> Scope<'_> {
        with_state(|state| {
            if state.enabled.get() {
                panic_any(Stop::Fault);
            }
            for value in &state.calls {
                value.set(0);
            }
            for value in &state.events {
                value.set(0);
            }
            state.steps.set(0);
            state.work.set(0);
            state.depth.set(0);
            state.maximum_depth.set(0);
            state.peak_stack.set(0);
            state.helper_depth.set(0);
            state.maximum_composed_depth.set(0);
            state.limits.set(limits);
            state
                .allowance
                .set(allowance.min(limits.work_per_declaration));
            let (top, capacity) = stack_bounds();
            if (capacity as u64) < limits.worker_stack_bytes {
                panic_any(Stop::Unavailable);
            }
            state.stack_top.set(top);
            state.stack_capacity.set(capacity);
            state.cancel.set(cancel as *const AtomicBool);
            state.enabled.set(true);
        });
        let guard = Scope {
            _borrow: PhantomData,
        };
        with_state(|state| state.add_work(limits.declaration_charge));
        guard
    }
}
impl Drop for Scope<'_> {
    fn drop(&mut self) {
        with_state(|state| {
            state.enabled.set(false);
            state.cancel.set(std::ptr::null());
        });
    }
}

#[inline(always)]
fn stack_pointer() -> usize {
    let pointer: usize;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::asm!("mov {}, sp", out(reg) pointer, options(nomem, nostack, preserves_flags));
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!("mov {}, rsp", out(reg) pointer, options(nomem, nostack, preserves_flags));
    }
    pointer
}

fn stack_bounds() -> (usize, usize) {
    #[cfg(target_os = "macos")]
    unsafe {
        unsafe extern "C" {
            fn pthread_self() -> usize;
            fn pthread_get_stackaddr_np(t: usize) -> *mut std::ffi::c_void;
            fn pthread_get_stacksize_np(t: usize) -> usize;
        }
        let thread = pthread_self();
        (
            pthread_get_stackaddr_np(thread) as usize,
            pthread_get_stacksize_np(thread),
        )
    }
    #[cfg(target_os = "linux")]
    unsafe {
        unsafe extern "C" {
            fn pthread_self() -> usize;
            fn pthread_getattr_np(t: usize, a: *mut std::ffi::c_void) -> i32;
            fn pthread_attr_getstack(
                a: *const std::ffi::c_void,
                b: *mut *mut std::ffi::c_void,
                n: *mut usize,
            ) -> i32;
            fn pthread_attr_destroy(a: *mut std::ffi::c_void) -> i32;
        }
        let mut attrs = [0usize; 16];
        let mut base = std::ptr::null_mut();
        let mut size = 0usize;
        if pthread_getattr_np(pthread_self(), attrs.as_mut_ptr().cast()) != 0 {
            panic_any(Stop::Unavailable);
        }
        let result = pthread_attr_getstack(attrs.as_ptr().cast(), &mut base, &mut size);
        let destroyed = pthread_attr_destroy(attrs.as_mut_ptr().cast());
        if result != 0 || destroyed != 0 {
            panic_any(Stop::Unavailable);
        }
        (base as usize + size, size)
    }
}
